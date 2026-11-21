//! Clipboard offers holding an image: `arboard` sets one format at a time, so a copy
//! with an image offers its PNG and its text itself, through `wl-clipboard-rs` on
//! Wayland and an `x11rb` selection owner on X11. Text-only copies stay on `arboard`.

use anyhow::Result;

use crate::graphics::{Pixels, SourceRect};

/// MIME type of the image offer.
#[cfg(target_os = "linux")]
const PNG_MIME: &str = "image/png";

/// What a copy puts in the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    /// Text only, through `arboard`.
    Text(String),
    /// A PNG, with the text when the selection holds any.
    Image { text: Option<String>, png: Vec<u8> },
}

/// Chooses what a copy offers: exactly one image in `images` → its PNG from `encode`,
/// with the non-empty `text`; no image, several images, or an image that cannot be
/// encoded → the text only. `None` when there is nothing to copy.
pub fn content(
    text: Option<String>,
    images: &[(u32, u32)],
    encode: impl FnOnce(u32, u32) -> Option<Vec<u8>>,
) -> Option<Content> {
    let text = text.filter(|text| !text.is_empty());
    if let [(image, placement)] = images
        && let Some(png) = encode(*image, *placement)
    {
        return Some(Content::Image { text, png });
    }
    text.map(Content::Text)
}

/// PNG of the part `source` (the whole image when `None`) of `pixels`. `None` when the
/// part is empty or the encoder fails.
pub fn encode_png(pixels: &Pixels, source: Option<SourceRect>) -> Option<Vec<u8>> {
    let rect = SourceRect::resolve(source, pixels.width, pixels.height)?;
    let stride = pixels.width as usize * 4;
    let mut rgba = Vec::with_capacity(rect.width as usize * rect.height as usize * 4);
    for row in rect.y..rect.y + rect.height {
        let start = row as usize * stride + rect.x as usize * 4;
        rgba.extend_from_slice(&pixels.rgba[start..start + rect.width as usize * 4]);
    }
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, rect.width, rect.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let result = encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&rgba).and(writer.finish()));
    if let Err(error) = result {
        log::warn!("PNG encoding failed: {error}");
        return None;
    }
    Some(png)
}

#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
mod x11;

/// Offers `png` as `image/png` in the clipboard, with `text` as UTF-8 text when given.
/// Wayland is tried first when `WAYLAND_DISPLAY` is set (compositors without the data
/// control protocol fall back to X11 through XWayland).
#[cfg(target_os = "linux")]
pub fn set_text_and_image(text: Option<String>, png: Vec<u8>) -> Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        match wayland::set(text.clone(), png.clone()) {
            Ok(()) => return Ok(()),
            Err(error) => log::debug!("Wayland clipboard unavailable, trying X11: {error}"),
        }
    }
    x11::set(text, png)
}

/// Copies a PNG image through the platform clipboard API.
#[cfg(target_os = "macos")]
pub fn set_image(clipboard: &mut arboard::Clipboard, png: &[u8]) -> Result<()> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info()?;
    let buffer_size = reader
        .output_buffer_size()
        .ok_or_else(|| anyhow::anyhow!("clipboard image is too large"))?;
    let mut buffer = vec![0; buffer_size];
    let info = reader.next_frame(&mut buffer)?;
    anyhow::ensure!(
        info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight,
        "clipboard image is not RGBA8"
    );
    buffer.truncate(info.buffer_size());
    clipboard.set_image(arboard::ImageData {
        width: usize::try_from(info.width)?,
        height: usize::try_from(info.height)?,
        bytes: buffer.into(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn pixels(width: u32, height: u32) -> Arc<Pixels> {
        let rgba = (0..width * height)
            .flat_map(|i| [i as u8, 0, 0, 255])
            .collect();
        Pixels::new(width, height, rgba).unwrap()
    }

    fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        buffer.truncate(info.buffer_size());
        (info.width, info.height, buffer)
    }

    #[test]
    fn text_alone_takes_the_text_path() {
        let content = content(Some("hello".into()), &[], |_, _| panic!("no image"));
        assert_eq!(content, Some(Content::Text("hello".into())));
    }

    #[test]
    fn one_image_is_offered_with_the_text() {
        let content = content(Some("hello".into()), &[(7, 3)], |image, placement| {
            assert_eq!((image, placement), (7, 3));
            Some(vec![1, 2])
        });
        assert_eq!(
            content,
            Some(Content::Image {
                text: Some("hello".into()),
                png: vec![1, 2]
            })
        );
    }

    #[test]
    fn an_image_alone_offers_no_text() {
        let content = content(Some(String::new()), &[(7, 3)], |_, _| Some(vec![1]));
        assert_eq!(
            content,
            Some(Content::Image {
                text: None,
                png: vec![1]
            })
        );
    }

    #[test]
    fn several_images_copy_the_text_only() {
        let content = content(Some("hi".into()), &[(7, 3), (9, 1)], |_, _| {
            panic!("no image")
        });
        assert_eq!(content, Some(Content::Text("hi".into())));
    }

    #[test]
    fn an_image_that_cannot_be_encoded_falls_back_to_the_text() {
        let content = content(Some("hi".into()), &[(7, 3)], |_, _| None);
        assert_eq!(content, Some(Content::Text("hi".into())));
    }

    #[test]
    fn nothing_to_copy_is_none() {
        assert_eq!(content(Some(String::new()), &[], |_, _| None), None);
        assert_eq!(content(None, &[(7, 3), (9, 1)], |_, _| None), None);
    }

    #[test]
    fn png_holds_the_whole_frame() {
        let png = encode_png(&pixels(3, 2), None).unwrap();
        let (width, height, rgba) = decode(&png);
        assert_eq!((width, height), (3, 2));
        assert_eq!(rgba, pixels(3, 2).rgba);
    }

    #[test]
    fn png_holds_only_the_shown_part() {
        let source = SourceRect {
            x: 1,
            y: 1,
            width: 2,
            height: 1,
        };
        let png = encode_png(&pixels(3, 2), Some(source)).unwrap();
        let (width, height, rgba) = decode(&png);
        assert_eq!((width, height), (2, 1));
        assert_eq!(rgba, [4, 0, 0, 255, 5, 0, 0, 255]);
    }

    #[test]
    fn png_of_an_empty_part_is_none() {
        let source = SourceRect {
            x: 5,
            y: 0,
            width: 2,
            height: 1,
        };
        assert_eq!(encode_png(&pixels(3, 2), Some(source)), None);
    }
}
