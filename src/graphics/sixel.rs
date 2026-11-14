//! Sixel decoder (DEC VT340 graphics), pure: the body of `ESC P <params> q <data> ESC \`
//! to RGBA pixels.
//!
//! Supported: color registers (`#`, RGB and HLS), repeat (`!`), graphics carriage return
//! (`$`), next line (`-`), raster attributes (`"`), and the background selected by the
//! second parameter. Pixels are square: the aspect ratio of `p1` and of the raster
//! attributes is ignored, as in the terminals sixel encoders target (xterm, foot, mlterm).

/// Largest width or height of a decoded image, in pixels: pixels beyond are dropped.
pub const MAX_SIDE: u32 = 8192;

/// Number of color registers.
const REGISTERS: usize = 1024;

/// The VT340 default palette, in percent, for the first 16 registers; the others start
/// black.
const DEFAULT_PALETTE: [[u8; 3]; 16] = [
    [0, 0, 0],
    [20, 20, 80],
    [80, 13, 13],
    [20, 80, 20],
    [80, 20, 80],
    [20, 80, 80],
    [80, 80, 20],
    [53, 53, 53],
    [26, 26, 26],
    [33, 33, 60],
    [60, 26, 26],
    [33, 60, 33],
    [60, 33, 60],
    [33, 60, 60],
    [60, 60, 33],
    [80, 80, 80],
];

/// A decoded sixel image: `width` x `height` straight RGBA pixels, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decodes the body of a sixel DCS (`<params>q<data>`). Returns `None` when the body has
/// no `q` or draws nothing.
///
/// Pixels no sixel sets are transparent when the second parameter is 1, else filled with
/// color register 0 as it is at the end of the image (the VT340 background).
pub fn decode(body: &[u8]) -> Option<Bitmap> {
    let q = body.iter().position(|&b| b == b'q')?;
    let params = numbers(&body[..q]);
    let transparent = params.get(1).copied() == Some(1);

    let mut decoder = Decoder::new();
    decoder.run(&body[q + 1..]);
    decoder.finish(transparent)
}

/// Parses `a;b;c` (missing numbers are 0, other bytes ignored).
fn numbers(bytes: &[u8]) -> Vec<u32> {
    let mut out = vec![0u32];
    for &b in bytes {
        match b {
            b'0'..=b'9' => {
                let last = out.last_mut().expect("never empty");
                *last = last.saturating_mul(10).saturating_add(u32::from(b - b'0'));
            }
            b';' => out.push(0),
            _ => {}
        }
    }
    out
}

/// The command being read, with the numbers read so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    None,
    Repeat,
    Color,
    Raster,
}

struct Decoder {
    palette: Vec<[u8; 3]>,
    color: usize,
    /// Painted pixels, `[r, g, b, 255]`, or all zero when unpainted; `stride` x `rows`.
    canvas: Vec<u8>,
    stride: u32,
    rows: u32,
    x: u32,
    /// Top pixel row of the current band of 6 rows.
    y: u32,
    /// Columns and bands any sixel reached, painted or not.
    extent_x: u32,
    extent_bands: u32,
    /// Lowest pixel row painted, plus one.
    painted_rows: u32,
    raster: (u32, u32),
    command: Command,
    args: Vec<u32>,
}

impl Decoder {
    fn new() -> Self {
        let mut palette = vec![[0u8; 3]; REGISTERS];
        for (register, percent) in palette.iter_mut().zip(DEFAULT_PALETTE) {
            *register = percent.map(percent_to_byte);
        }
        Self {
            palette,
            color: 0,
            canvas: Vec::new(),
            stride: 0,
            rows: 0,
            x: 0,
            y: 0,
            extent_x: 0,
            extent_bands: 0,
            painted_rows: 0,
            raster: (0, 0),
            command: Command::None,
            args: Vec::new(),
        }
    }

    fn run(&mut self, data: &[u8]) {
        for &b in data {
            match b {
                b'0'..=b'9' if self.command != Command::None => {
                    let last = self.args.last_mut().expect("a command has an argument");
                    *last = last.saturating_mul(10).saturating_add(u32::from(b - b'0'));
                }
                b';' if self.command != Command::None => self.args.push(0),
                b'?'..=b'~' => {
                    let count = match self.command {
                        Command::Repeat => self.args[0].max(1),
                        _ => 1,
                    };
                    self.end_command();
                    self.sixel(b - b'?', count);
                }
                b'!' | b'#' | b'"' => {
                    self.end_command();
                    self.command = match b {
                        b'!' => Command::Repeat,
                        b'#' => Command::Color,
                        _ => Command::Raster,
                    };
                    self.args.clear();
                    self.args.push(0);
                }
                b'$' => {
                    self.end_command();
                    self.x = 0;
                }
                b'-' => {
                    self.end_command();
                    self.x = 0;
                    self.y = self.y.saturating_add(6);
                }
                // Line breaks and other bytes between commands are ignored.
                _ => {}
            }
        }
        self.end_command();
    }

    /// Applies a color or raster command once its numbers are read.
    fn end_command(&mut self) {
        match self.command {
            Command::Color => {
                let register = self.args[0] as usize % REGISTERS;
                if let [_, space, a, b, c, ..] = self.args[..] {
                    let rgb = match space {
                        1 => Some(hls_to_rgb(a, b, c)),
                        2 => Some([a, b, c].map(|v| percent_to_byte(v.min(100) as u8))),
                        _ => None,
                    };
                    if let Some(rgb) = rgb {
                        self.palette[register] = rgb;
                    }
                }
                self.color = register;
            }
            Command::Raster => {
                if let [_, _, width, height, ..] = self.args[..] {
                    self.raster = (width.min(MAX_SIDE), height.min(MAX_SIDE));
                }
            }
            Command::Repeat | Command::None => {}
        }
        self.command = Command::None;
    }

    /// Draws `count` columns of the sixel `bits` (bit 0 at the top) at the cursor.
    fn sixel(&mut self, bits: u8, count: u32) {
        let start = self.x;
        self.x = self.x.saturating_add(count);
        let end = self.x.min(MAX_SIDE);
        if start >= end || self.y >= MAX_SIDE {
            return;
        }
        self.extent_x = self.extent_x.max(end);
        self.extent_bands = self.extent_bands.max(self.y / 6 + 1);
        if bits == 0 {
            return;
        }
        let last_bit = 7 - bits.leading_zeros();
        let bottom = (self.y + last_bit + 1).min(MAX_SIDE);
        self.painted_rows = self.painted_rows.max(bottom);
        self.grow(end, bottom);

        let [r, g, b] = self.palette[self.color];
        let pixel = [r, g, b, 255];
        for bit in 0..6 {
            let row = self.y + bit;
            if bits & (1 << bit) == 0 || row >= MAX_SIDE {
                continue;
            }
            let line = (row * self.stride) as usize * 4;
            for x in start..end {
                let at = line + x as usize * 4;
                self.canvas[at..at + 4].copy_from_slice(&pixel);
            }
        }
    }

    /// Makes the canvas at least `width` x `height`, doubling to keep growth cheap.
    fn grow(&mut self, width: u32, height: u32) {
        if width <= self.stride && height <= self.rows {
            return;
        }
        let stride = if width > self.stride {
            width.max(self.stride * 2).min(MAX_SIDE)
        } else {
            self.stride
        };
        let rows = if height > self.rows {
            height.max(self.rows * 2).min(MAX_SIDE)
        } else {
            self.rows
        };
        let mut canvas = vec![0u8; stride as usize * rows as usize * 4];
        let old_line = self.stride as usize * 4;
        for row in 0..self.rows as usize {
            let from = row * old_line;
            let to = row * stride as usize * 4;
            canvas[to..to + old_line].copy_from_slice(&self.canvas[from..from + old_line]);
        }
        self.canvas = canvas;
        self.stride = stride;
        self.rows = rows;
    }

    /// The image: as wide as the raster attributes or the columns reached, as high as
    /// the raster attributes or the painted rows (whole bands when filled).
    fn finish(self, transparent: bool) -> Option<Bitmap> {
        let width = self.raster.0.max(self.extent_x);
        let drawn = if transparent || self.raster.1 > 0 {
            self.painted_rows
        } else {
            (self.extent_bands * 6).min(MAX_SIDE)
        };
        let height = self.raster.1.max(drawn);
        if width == 0 || height == 0 {
            return None;
        }

        let background = if transparent {
            [0, 0, 0, 0]
        } else {
            let [r, g, b] = self.palette[0];
            [r, g, b, 255]
        };
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for row in 0..height {
            for x in 0..width {
                let pixel = if row < self.rows && x < self.stride {
                    let at = (row * self.stride + x) as usize * 4;
                    &self.canvas[at..at + 4]
                } else {
                    &[0u8; 4][..]
                };
                if pixel[3] == 0 {
                    rgba.extend_from_slice(&background);
                } else {
                    rgba.extend_from_slice(pixel);
                }
            }
        }
        Some(Bitmap {
            width,
            height,
            rgba,
        })
    }
}

fn percent_to_byte(percent: u8) -> u8 {
    ((u32::from(percent.min(100)) * 255 + 50) / 100) as u8
}

/// DEC HLS (hue 0 is blue, 120 red, 240 green; lightness and saturation in percent) to
/// RGB.
fn hls_to_rgb(hue: u32, lightness: u32, saturation: u32) -> [u8; 3] {
    let h = f64::from((hue % 360 + 240) % 360) / 360.0;
    let l = f64::from(lightness.min(100)) / 100.0;
    let s = f64::from(saturation.min(100)) / 100.0;
    if s == 0.0 {
        let v = (l * 255.0).round() as u8;
        return [v; 3];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |t: f64| {
        let t = t.rem_euclid(1.0);
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v * 255.0).round() as u8
    };
    [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    fn pixel(bitmap: &Bitmap, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * bitmap.width + x) * 4) as usize;
        bitmap.rgba[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn rgb_register_paints_the_set_bits_top_down() {
        // `A` = 0b000010: second row only; `~` = all six rows.
        let image = decode(b"0;1;0q#1;2;100;0;0#1A~").unwrap();
        assert_eq!((image.width, image.height), (2, 6));
        assert_eq!(pixel(&image, 0, 0), CLEAR);
        assert_eq!(pixel(&image, 0, 1), RED);
        assert_eq!(pixel(&image, 0, 2), CLEAR);
        for y in 0..6 {
            assert_eq!(pixel(&image, 1, y), RED);
        }
    }

    #[test]
    fn hls_register_uses_the_dec_hue_origin() {
        // DEC hue 120 is red, 240 green, 0 blue.
        assert_eq!(hls_to_rgb(120, 50, 100), [255, 0, 0]);
        assert_eq!(hls_to_rgb(240, 50, 100), [0, 255, 0]);
        assert_eq!(hls_to_rgb(0, 50, 100), [0, 0, 255]);
        assert_eq!(hls_to_rgb(0, 100, 0), [255, 255, 255]);
        let image = decode(b"0;1q#3;1;240;50;100#3@").unwrap();
        assert_eq!(pixel(&image, 0, 0), GREEN);
    }

    #[test]
    fn selecting_a_register_keeps_its_color_and_defaults_follow_the_vt340() {
        let image = decode(b"0;1q#2;2;0;100;0#5@#2@").unwrap();
        // Register 5 keeps its VT340 default (20%, 80%, 80%).
        assert_eq!(pixel(&image, 0, 0), [51, 204, 204, 255]);
        assert_eq!(pixel(&image, 1, 0), GREEN);
    }

    #[test]
    fn repeat_draws_count_columns_and_zero_means_one() {
        let image = decode(b"0;1q#1;2;100;0;0!3@!0@").unwrap();
        assert_eq!(image.width, 4);
        for x in 0..4 {
            assert_eq!(pixel(&image, x, 0), RED);
        }
    }

    #[test]
    fn carriage_return_and_next_line_move_the_cursor() {
        // Red top row, then `$` back over it in green on the second row, then `-` to
        // the next band.
        let image = decode(b"0;1q#1;2;100;0;0#2;2;0;100;0#1@@$#2AA-#1@").unwrap();
        assert_eq!((image.width, image.height), (2, 7));
        assert_eq!(pixel(&image, 0, 0), RED);
        assert_eq!(pixel(&image, 1, 1), GREEN);
        assert_eq!(pixel(&image, 0, 6), RED);
        assert_eq!(pixel(&image, 1, 6), CLEAR);
    }

    #[test]
    fn raster_attributes_size_the_image() {
        // 4 x 3 raster, one painted pixel: the raster sets the size.
        let image = decode(b"0;1q\"1;1;4;3#1;2;100;0;0@").unwrap();
        assert_eq!((image.width, image.height), (4, 3));
        assert_eq!(pixel(&image, 0, 0), RED);
        assert_eq!(pixel(&image, 3, 2), CLEAR);
    }

    #[test]
    fn raster_height_trims_the_last_band_and_drawing_beyond_extends_it() {
        // Filled background: a 2 x 4 raster trims the 6-row band to 4 rows.
        let image = decode(b"q\"1;1;2;4#1;2;100;0;0@@").unwrap();
        assert_eq!((image.width, image.height), (2, 4));
        // Pixels drawn past the raster grow the image.
        let image = decode(b"q\"1;1;1;1#1;2;100;0;0~~").unwrap();
        assert_eq!((image.width, image.height), (2, 6));
    }

    #[test]
    fn filled_background_uses_register_zero_and_whole_bands() {
        let image = decode(b"0;0q#0;2;0;0;100#1;2;100;0;0#1@").unwrap();
        assert_eq!((image.width, image.height), (1, 6));
        assert_eq!(pixel(&image, 0, 0), RED);
        assert_eq!(pixel(&image, 0, 5), [0, 0, 255, 255]);
        // No parameter at all is the filled mode too.
        let image = decode(b"q#1;2;100;0;0@").unwrap();
        assert_eq!(pixel(&image, 0, 1), [0, 0, 0, 255]);
    }

    #[test]
    fn transparent_background_keeps_unset_pixels_clear() {
        let image = decode(b"0;1q#1;2;100;0;0@-@").unwrap();
        assert_eq!((image.width, image.height), (1, 7));
        assert_eq!(pixel(&image, 0, 3), CLEAR);
        assert_eq!(pixel(&image, 0, 6), RED);
    }

    #[test]
    fn line_breaks_between_commands_are_ignored() {
        let image = decode(b"0;1q#1;2;\r\n100;0;0\n#1@\r\n@").unwrap();
        assert_eq!(image.width, 2);
        assert_eq!(pixel(&image, 1, 0), RED);
    }

    #[test]
    fn empty_or_malformed_bodies_decode_to_nothing() {
        assert_eq!(decode(b"0;1;0"), None);
        assert_eq!(decode(b"0;1q"), None);
        assert_eq!(decode(b"0;1q#1;2;100;0;0"), None);
    }

    #[test]
    fn huge_repeats_are_clipped() {
        let image = decode(b"0;1q!99999999@").unwrap();
        assert_eq!((image.width, image.height), (MAX_SIDE, 1));
    }
}
