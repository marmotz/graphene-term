//! Decoding of the transmitted data into RGBA pixels: raw RGB(A) or PNG, optionally zlib-compressed.

use super::*;

/// Decodes the bytes of an image, as [`load`] returns them, to RGBA pixels.
pub fn decode(command: &Command, bytes: Vec<u8>) -> Result<Arc<Pixels>, Error> {
    let bytes = match command.compression {
        0 => bytes,
        b'z' => inflate(&bytes)?,
        other => {
            return Err(Error::invalid(format!(
                "unknown compression: {}",
                other as char
            )));
        }
    };
    match command.format {
        100 => decode_png(&bytes),
        24 | 32 => decode_raw(command, &bytes),
        other => Err(Error::invalid(format!("unknown format: {other}"))),
    }
}

pub(super) fn inflate(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(bytes)
        .take(MAX_DATA as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|err| Error::invalid(format!("bad zlib data: {err}")))?;
    if out.len() > MAX_DATA {
        return Err(Error::too_big());
    }
    Ok(out)
}

pub(super) fn decode_png(bytes: &[u8]) -> Result<Arc<Pixels>, Error> {
    let bad = |err: png::DecodingError| Error::invalid(format!("bad PNG data: {err}"));
    let mut decoder =
        png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: MAX_DATA });
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(bad)?;
    let size = reader.output_buffer_size().ok_or_else(Error::too_big)?;
    if size > MAX_DATA {
        return Err(Error::too_big());
    }
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(bad)?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => rgb_to_rgba(&buf),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[g, a]| [g, g, g, a])
            .collect(),
        png::ColorType::Indexed => return Err(Error::invalid("unexpanded PNG palette")),
    };
    Pixels::new(info.width, info.height, rgba).ok_or_else(|| Error::invalid("empty PNG image"))
}

pub(super) fn rgb_to_rgba(rgb: &[u8]) -> Vec<u8> {
    rgb.as_chunks::<3>()
        .0
        .iter()
        .flat_map(|&[r, g, b]| [r, g, b, 255])
        .collect()
}

pub(super) fn decode_raw(command: &Command, bytes: &[u8]) -> Result<Arc<Pixels>, Error> {
    let (width, height) = (command.width, command.height);
    if width == 0 || height == 0 {
        return Err(Error::invalid(
            "the image width and height (s, v) are required",
        ));
    }
    let channels = if command.format == 24 { 3 } else { 4 };
    let needed = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(channels))
        .filter(|&needed| needed <= MAX_DATA)
        .ok_or_else(Error::too_big)?;
    if bytes.len() < needed {
        return Err(Error::new(
            "ENODATA",
            format!(
                "insufficient image data: {} bytes, {needed} needed",
                bytes.len()
            ),
        ));
    }
    let bytes = &bytes[..needed];
    let rgba = if channels == 4 {
        bytes.to_vec()
    } else {
        rgb_to_rgba(bytes)
    };
    Pixels::new(width, height, rgba).ok_or_else(|| Error::invalid("empty image"))
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    #[test]
    fn raw_rgb_and_rgba_are_decoded() {
        let rgb = decode(&command("f=24,s=2,v=1"), vec![1, 2, 3, 4, 5, 6]).unwrap();
        assert_eq!((rgb.width, rgb.height), (2, 1));
        assert_eq!(rgb.rgba, [1, 2, 3, 255, 4, 5, 6, 255]);

        // Extra bytes are ignored.
        let rgba = decode(&command("f=32,s=1,v=2"), vec![9; 10]).unwrap();
        assert_eq!(rgba.rgba, [9; 8]);
    }

    #[test]
    fn raw_data_needs_its_size_and_enough_bytes() {
        let err = decode(&command("f=32,s=2"), vec![0; 8]).unwrap_err();
        assert_eq!(err.code, "EINVAL");
        let err = decode(&command("f=32,s=2,v=2"), vec![0; 15]).unwrap_err();
        assert_eq!(err.code, "ENODATA");
        let err = decode(&command("f=32,s=100000,v=100000"), vec![0; 4]).unwrap_err();
        assert_eq!(err.code, "EFBIG");
        let err = decode(&command("f=42,s=1,v=1"), vec![0; 4]).unwrap_err();
        assert_eq!(err.code, "EINVAL");
    }

    #[test]
    fn png_is_decoded_to_rgba_whatever_its_color_type() {
        let cases = [
            (
                png::ColorType::Rgba,
                vec![1, 2, 3, 4, 5, 6, 7, 8],
                vec![1, 2, 3, 4, 5, 6, 7, 8],
            ),
            (
                png::ColorType::Rgb,
                vec![1, 2, 3, 4, 5, 6],
                vec![1, 2, 3, 255, 4, 5, 6, 255],
            ),
            (
                png::ColorType::Grayscale,
                vec![7, 9],
                vec![7, 7, 7, 255, 9, 9, 9, 255],
            ),
            (
                png::ColorType::GrayscaleAlpha,
                vec![7, 1, 9, 2],
                vec![7, 7, 7, 1, 9, 9, 9, 2],
            ),
        ];
        for (color, data, expected) in cases {
            let pixels = decode(&command("f=100"), png_bytes(2, 1, color, &data)).unwrap();
            assert_eq!((pixels.width, pixels.height), (2, 1));
            assert_eq!(pixels.rgba, expected, "{color:?}");
        }

        let err = decode(&command("f=100"), b"not a png".to_vec()).unwrap_err();
        assert_eq!(err.code, "EINVAL");
    }

    #[test]
    fn zlib_compressed_data_is_inflated_first() {
        let raw = decode(&command("f=24,s=1,v=1,o=z"), zlib(&[1, 2, 3])).unwrap();
        assert_eq!(raw.rgba, [1, 2, 3, 255]);

        let data = png_bytes(1, 1, png::ColorType::Rgba, &[4, 5, 6, 7]);
        let png = decode(&command("f=100,o=z"), zlib(&data)).unwrap();
        assert_eq!(png.rgba, [4, 5, 6, 7]);

        let err = decode(&command("f=24,s=1,v=1,o=z"), vec![1, 2, 3]).unwrap_err();
        assert_eq!(err.code, "EINVAL");
        let err = decode(&command("f=24,s=1,v=1,o=x"), vec![1, 2, 3]).unwrap_err();
        assert_eq!(err.code, "EINVAL");
    }

    fn png_bytes(width: u32, height: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        writer.finish().unwrap();
        out
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn command(control: &str) -> Command {
        parse(control.as_bytes()).unwrap().0
    }
}
