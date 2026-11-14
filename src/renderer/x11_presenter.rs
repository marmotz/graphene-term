//! X11 ARGB presentation for GPU backends that expose only opaque surfaces.

use anyhow::{Context, ensure};
use wgpu::rwh::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use winit::event_loop::OwnedDisplayHandle;
use winit::window::Window;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::render::ConnectionExt as RenderConnectionExt;
use x11rb::protocol::xproto::{ConnectionExt, CreateGCAux, ImageFormat, ImageOrder, VisualClass};

pub struct X11Presenter {
    connection: x11rb::rust_connection::RustConnection,
    window: u32,
    gc: u32,
    depth: u8,
    bits_per_pixel: u8,
    scanline_pad: u8,
    image_byte_order: ImageOrder,
    red: Channel,
    green: Channel,
    blue: Channel,
    alpha: Channel,
    pixels: Vec<u8>,
}

#[derive(Clone, Copy)]
struct Channel {
    shift: u16,
    mask: u16,
}

impl Channel {
    fn pack(self, value: u8) -> u32 {
        ((u32::from(value) * u32::from(self.mask) + 127) / 255) << self.shift
    }
}

impl X11Presenter {
    /// Uses the existing winit ARGB window. `Ok(None)` means the native window is not X11.
    pub fn new(window: &Window, display: &OwnedDisplayHandle) -> anyhow::Result<Option<Self>> {
        let raw_display = display.display_handle()?.as_raw();
        let screen = match raw_display {
            RawDisplayHandle::Xlib(handle) => handle.screen,
            RawDisplayHandle::Xcb(handle) => handle.screen,
            _ => return Ok(None),
        };
        let (window, visual) = match window.window_handle()?.as_raw() {
            RawWindowHandle::Xlib(handle) => {
                let window = u32::try_from(handle.window).context("X11 window ID overflow")?;
                let visual = u32::try_from(handle.visual_id).context("X11 visual ID overflow")?;
                (window, visual)
            }
            RawWindowHandle::Xcb(handle) => (
                handle.window.get(),
                handle
                    .visual_id
                    .context("X11 window has no visual ID")?
                    .get(),
            ),
            _ => return Ok(None),
        };

        let (connection, default_screen) = x11rb::connect(None)?;
        let requested_screen = usize::try_from(screen).context("invalid X11 screen number")?;
        let screen_num = if requested_screen < connection.setup().roots.len() {
            requested_screen
        } else {
            default_screen
        };
        let screen = connection
            .setup()
            .roots
            .get(screen_num)
            .context("X11 screen is unavailable")?;
        let (depth, visual_type) = screen
            .allowed_depths
            .iter()
            .find_map(|depth| {
                depth
                    .visuals
                    .iter()
                    .find(|candidate| candidate.visual_id == visual)
                    .map(|visual| (depth.depth, *visual))
            })
            .context("winit's X11 visual is no longer available")?;
        ensure!(
            depth == 32 && visual_type.class == VisualClass::TRUE_COLOR,
            "winit did not create a depth-32 TrueColor window"
        );

        let formats = connection.render_query_pict_formats()?.reply()?;
        let picture_visual = formats
            .screens
            .get(screen_num)
            .context("XRender screen is unavailable")?
            .depths
            .iter()
            .flat_map(|depth| &depth.visuals)
            .find(|candidate| candidate.visual == visual)
            .context("XRender does not expose a format for the window visual")?;
        let picture_format = formats
            .formats
            .iter()
            .find(|candidate| candidate.id == picture_visual.format)
            .context("XRender visual format is unavailable")?;
        ensure!(
            picture_format.direct.alpha_mask != 0,
            "X11 window visual has no per-pixel alpha channel"
        );

        let pixmap_format = connection
            .setup()
            .pixmap_formats
            .iter()
            .find(|format| format.depth == depth)
            .context("X11 server has no pixmap format for the ARGB visual")?;
        ensure!(
            pixmap_format.bits_per_pixel == 32,
            "X11 ARGB visual uses unsupported {}-bit pixels",
            pixmap_format.bits_per_pixel
        );
        let bits_per_pixel = pixmap_format.bits_per_pixel;
        let scanline_pad = pixmap_format.scanline_pad;

        let gc = connection.generate_id()?;
        connection
            .create_gc(gc, window, &CreateGCAux::new())?
            .check()?;
        log::info!(
            "using X11 ARGB presentation (visual 0x{visual:x}, alpha mask 0x{:x})",
            picture_format.direct.alpha_mask
        );

        Ok(Some(Self {
            image_byte_order: connection.setup().image_byte_order,
            connection,
            window,
            gc,
            depth,
            bits_per_pixel,
            scanline_pad,
            red: Channel {
                shift: picture_format.direct.red_shift,
                mask: picture_format.direct.red_mask,
            },
            green: Channel {
                shift: picture_format.direct.green_shift,
                mask: picture_format.direct.green_mask,
            },
            blue: Channel {
                shift: picture_format.direct.blue_shift,
                mask: picture_format.direct.blue_mask,
            },
            alpha: Channel {
                shift: picture_format.direct.alpha_shift,
                mask: picture_format.direct.alpha_mask,
            },
            pixels: Vec::new(),
        }))
    }

    /// Presents tightly or GPU-padded rows from an RGBA/BGRA texture.
    pub fn present(
        &mut self,
        source: &[u8],
        width: u32,
        height: u32,
        source_bytes_per_row: u32,
        format: wgpu::TextureFormat,
    ) -> anyhow::Result<()> {
        let pixel_bytes = usize::from(self.bits_per_pixel / 8);
        ensure!(pixel_bytes == 4, "X11 ARGB requires 4-byte pixels");
        let unpadded_row_bytes = usize::try_from(width)? * pixel_bytes;
        let row_bits = unpadded_row_bytes * 8;
        let pad_bits = usize::from(self.scanline_pad);
        let destination_bytes_per_row = row_bits.div_ceil(pad_bits) * pad_bits / 8;
        self.pixels
            .resize(destination_bytes_per_row * usize::try_from(height)?, 0);

        let source_is_bgra = matches!(
            format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        for y in 0..usize::try_from(height)? {
            let source_row = &source[y * source_bytes_per_row as usize
                ..y * source_bytes_per_row as usize + unpadded_row_bytes];
            let destination_row = &mut self.pixels
                [y * destination_bytes_per_row..(y + 1) * destination_bytes_per_row];
            let (source_pixels, _) = source_row.as_chunks::<4>();
            let (destination_pixels, _) = destination_row.as_chunks_mut::<4>();
            for (source_pixel, destination_pixel) in
                source_pixels.iter().zip(destination_pixels.iter_mut())
            {
                let (red, green, blue, alpha) = if source_is_bgra {
                    (
                        source_pixel[2],
                        source_pixel[1],
                        source_pixel[0],
                        source_pixel[3],
                    )
                } else {
                    (
                        source_pixel[0],
                        source_pixel[1],
                        source_pixel[2],
                        source_pixel[3],
                    )
                };
                let pixel = self.red.pack(red)
                    | self.green.pack(green)
                    | self.blue.pack(blue)
                    | self.alpha.pack(alpha);
                let bytes = match self.image_byte_order {
                    ImageOrder::LSB_FIRST => pixel.to_le_bytes(),
                    ImageOrder::MSB_FIRST => pixel.to_be_bytes(),
                    _ => anyhow::bail!("unsupported X11 image byte order"),
                };
                destination_pixel.copy_from_slice(&bytes);
            }
        }

        let rows_per_request = (self.connection.maximum_request_bytes().saturating_sub(64)
            / destination_bytes_per_row)
            .max(1);
        for first_row in (0..usize::try_from(height)?).step_by(rows_per_request) {
            let rows = rows_per_request.min(usize::try_from(height)? - first_row);
            let start = first_row * destination_bytes_per_row;
            let end = start + rows * destination_bytes_per_row;
            self.connection.put_image(
                ImageFormat::Z_PIXMAP,
                self.window,
                self.gc,
                u16::try_from(width).context("X11 window is wider than the protocol limit")?,
                u16::try_from(rows)?,
                0,
                i16::try_from(first_row).context("X11 window is taller than the protocol limit")?,
                0,
                self.depth,
                &self.pixels[start..end],
            )?;
        }
        self.connection.flush()?;
        Ok(())
    }
}

impl Drop for X11Presenter {
    fn drop(&mut self) {
        let _ = self.connection.free_gc(self.gc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_pack_uses_the_visual_shift_and_mask() {
        assert_eq!(
            Channel {
                shift: 16,
                mask: 0xff
            }
            .pack(0x80),
            0x800000
        );
        assert_eq!(
            Channel {
                shift: 24,
                mask: 0xff
            }
            .pack(0xff),
            0xff000000
        );
    }
}
