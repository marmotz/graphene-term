//! Drawing into a texture read back by the CPU, and packing its pixels for a window
//! system: the X11 ARGB presentation, and the benchmarks measuring it.

use std::sync::mpsc;

use anyhow::{Context, ensure};

/// End of the mapping of a readback buffer, sent by its callback.
type Mapped = mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>;

/// Texture a frame is drawn into, and the buffer it is read back with.
pub(super) struct OffscreenTarget {
    pub(super) texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
    pub(super) readback: wgpu::Buffer,
    pub(super) format: wgpu::TextureFormat,
    pub(super) size: (u32, u32),
    pub(super) bytes_per_row: u32,
    /// Read started by [`Self::start_read`], not finished yet.
    pending: Option<Mapped>,
}

impl OffscreenTarget {
    pub(super) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> anyhow::Result<Self> {
        let width = width.max(1);
        let height = height.max(1);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen render target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let unpadded_bytes_per_row = width
            .checked_mul(4)
            .context("offscreen row size overflow")?;
        let bytes_per_row = unpadded_bytes_per_row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offscreen readback"),
            size: u64::from(bytes_per_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Ok(Self {
            texture,
            view,
            readback,
            format,
            size: (width, height),
            bytes_per_row,
            pending: None,
        })
    }

    /// Copies the texture to the readback buffer at the end of `encoder`, submits it
    /// and returns without waiting for the GPU: [`Self::finish_read`] reads the rows.
    /// Fails while the previous read is not finished.
    pub(super) fn start_read(
        &mut self,
        queue: &wgpu::Queue,
        mut encoder: wgpu::CommandEncoder,
    ) -> anyhow::Result<()> {
        ensure!(
            self.pending.is_none(),
            "the previous GPU readback is not finished"
        );
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.bytes_per_row),
                    rows_per_image: Some(self.size.1),
                },
            },
            wgpu::Extent3d {
                width: self.size.0,
                height: self.size.1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let (send, receive) = mpsc::sync_channel(1);
        self.readback
            .map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = send.send(result);
            });
        self.pending = Some(receive);
        Ok(())
    }

    /// Whether a read started by [`Self::start_read`] is not finished.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(super) fn is_reading(&self) -> bool {
        self.pending.is_some()
    }

    /// Calls `read` with the rows (`bytes_per_row` apart) of the read started by
    /// [`Self::start_read`] once the GPU copied them, waiting for it when `wait`.
    /// `None` when no read is pending, or without `wait` while the GPU is not done.
    pub(super) fn finish_read<R>(
        &mut self,
        device: &wgpu::Device,
        wait: bool,
        read: impl FnOnce(&[u8]) -> R,
    ) -> anyhow::Result<Option<R>> {
        let Some(pending) = &self.pending else {
            return Ok(None);
        };
        device.poll(if wait {
            wgpu::PollType::wait_indefinitely()
        } else {
            wgpu::PollType::Poll
        })?;
        let mapped = match pending.try_recv() {
            Ok(mapped) => mapped,
            Err(mpsc::TryRecvError::Empty) if !wait => return Ok(None),
            Err(_) => {
                self.pending = None;
                anyhow::bail!("GPU readback callback was dropped");
            }
        };
        self.pending = None;
        mapped?;
        let rows = self.readback.get_mapped_range(..)?;
        let result = read(&rows);
        drop(rows);
        self.readback.unmap();
        Ok(Some(result))
    }
}

/// Position and width of a color channel in a packed pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Channel {
    pub(super) shift: u16,
    pub(super) mask: u16,
}

impl Channel {
    fn pack(self, value: u8) -> u32 {
        ((u32::from(value) * u32::from(self.mask) + 127) / 255) << self.shift
    }
}

/// How a window system lays out the 32-bit pixels of an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PixelLayout {
    pub(super) red: Channel,
    pub(super) green: Channel,
    pub(super) blue: Channel,
    pub(super) alpha: Channel,
    /// Whether the most significant byte of a pixel comes first.
    pub(super) big_endian: bool,
    /// Bits each row is padded to.
    pub(super) scanline_pad: u8,
}

impl PixelLayout {
    /// Usual X11 ARGB visual: 8 bits per channel, alpha in the top byte, little endian.
    #[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
    pub(super) const ARGB32: Self = Self {
        red: Channel {
            shift: 16,
            mask: 0xff,
        },
        green: Channel {
            shift: 8,
            mask: 0xff,
        },
        blue: Channel {
            shift: 0,
            mask: 0xff,
        },
        alpha: Channel {
            shift: 24,
            mask: 0xff,
        },
        big_endian: false,
        scanline_pad: 32,
    };

    /// Packs the `width` x `height` RGBA or BGRA pixels of `source`, whose rows are
    /// `source_bytes_per_row` apart, into `out`. Returns the bytes per row of `out`.
    pub(super) fn pack(
        &self,
        source: &[u8],
        width: u32,
        height: u32,
        source_bytes_per_row: u32,
        format: wgpu::TextureFormat,
        out: &mut Vec<u8>,
    ) -> anyhow::Result<usize> {
        let width = usize::try_from(width)?;
        let height = usize::try_from(height)?;
        let source_bytes_per_row = usize::try_from(source_bytes_per_row)?;
        let unpadded_row_bytes = width * 4;
        let needed = match height {
            0 => 0,
            _ => source_bytes_per_row * (height - 1) + unpadded_row_bytes,
        };
        ensure!(
            source_bytes_per_row >= unpadded_row_bytes && source.len() >= needed,
            "pixel source is smaller than {width}x{height}"
        );
        let pad_bits = usize::from(self.scanline_pad.max(8));
        let destination_bytes_per_row = (unpadded_row_bytes * 8).div_ceil(pad_bits) * pad_bits / 8;
        out.resize(destination_bytes_per_row * height, 0);

        let source_is_bgra = matches!(
            format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let rows = (0..height).map(|y| {
            (
                y * source_bytes_per_row..y * source_bytes_per_row + unpadded_row_bytes,
                y * destination_bytes_per_row..(y + 1) * destination_bytes_per_row,
            )
        });
        match self.byte_order(source_is_bgra) {
            // The source already holds the pixels as the window system wants them.
            Some([0, 1, 2, 3]) => {
                for (source_row, destination_row) in rows {
                    out[destination_row][..unpadded_row_bytes].copy_from_slice(&source[source_row]);
                }
            }
            Some(order) => {
                for (source_row, destination_row) in rows {
                    let (source_pixels, _) = source[source_row].as_chunks::<4>();
                    let (destination_pixels, _) = out[destination_row].as_chunks_mut::<4>();
                    for (from, to) in source_pixels.iter().zip(destination_pixels) {
                        *to = order.map(|index| from[index]);
                    }
                }
            }
            None => {
                for (source_row, destination_row) in rows {
                    let (source_pixels, _) = source[source_row].as_chunks::<4>();
                    let (destination_pixels, _) = out[destination_row].as_chunks_mut::<4>();
                    for (from, to) in source_pixels.iter().zip(destination_pixels) {
                        *to = self.pack_pixel(*from, source_is_bgra);
                    }
                }
            }
        }
        Ok(destination_bytes_per_row)
    }

    /// Bytes of a source pixel (RGBA, or BGRA when `bgra`) each byte of a packed pixel
    /// takes, when every channel is 8 bits wide on a byte boundary: packing is then a
    /// byte shuffle. `None` for other layouts.
    fn byte_order(&self, bgra: bool) -> Option<[usize; 4]> {
        let sources = if bgra { [2, 1, 0, 3] } else { [0, 1, 2, 3] };
        let mut order = [usize::MAX; 4];
        for (channel, source) in [self.red, self.green, self.blue, self.alpha]
            .into_iter()
            .zip(sources)
        {
            if channel.mask != 0xff || channel.shift % 8 != 0 || channel.shift > 24 {
                return None;
            }
            let byte = usize::from(channel.shift / 8);
            let byte = if self.big_endian { 3 - byte } else { byte };
            if order[byte] != usize::MAX {
                return None;
            }
            order[byte] = source;
        }
        Some(order)
    }

    /// Packs one source pixel (RGBA, or BGRA when `bgra`) with any channel layout.
    fn pack_pixel(&self, pixel: [u8; 4], bgra: bool) -> [u8; 4] {
        let [red, green, blue, alpha] = if bgra {
            [pixel[2], pixel[1], pixel[0], pixel[3]]
        } else {
            pixel
        };
        let packed = self.red.pack(red)
            | self.green.pack(green)
            | self.blue.pack(blue)
            | self.alpha.pack(alpha);
        if self.big_endian {
            packed.to_be_bytes()
        } else {
            packed.to_le_bytes()
        }
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
        assert_eq!(
            Channel {
                shift: 0,
                mask: 0x1f
            }
            .pack(0xff),
            0x1f
        );
    }

    #[test]
    fn argb32_packs_rgba_and_bgra_rows_into_little_endian_pixels() {
        // Two pixels per row, rows padded to 12 bytes.
        let source = [
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0, 0, 0, 0, //
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0, 0, 0, 0,
        ];
        let mut out = Vec::new();
        let layout = PixelLayout::ARGB32;
        let row = layout
            .pack(&source, 2, 2, 12, wgpu::TextureFormat::Rgba8Unorm, &mut out)
            .unwrap();
        assert_eq!(row, 8);
        assert_eq!(
            out,
            [
                0x33, 0x22, 0x11, 0x44, 0x77, 0x66, 0x55, 0x88, //
                0x03, 0x02, 0x01, 0x04, 0x07, 0x06, 0x05, 0x08,
            ]
        );
        layout
            .pack(
                &source,
                2,
                2,
                12,
                wgpu::TextureFormat::Bgra8UnormSrgb,
                &mut out,
            )
            .unwrap();
        assert_eq!(&out[..8], [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
    }

    #[test]
    fn pack_honours_big_endian_and_scanline_padding() {
        let layout = PixelLayout {
            big_endian: true,
            scanline_pad: 64,
            ..PixelLayout::ARGB32
        };
        let mut out = Vec::new();
        let row = layout
            .pack(
                &[1, 2, 3, 4],
                1,
                1,
                4,
                wgpu::TextureFormat::Rgba8Unorm,
                &mut out,
            )
            .unwrap();
        assert_eq!(row, 8);
        assert_eq!(out, [4, 1, 2, 3, 0, 0, 0, 0]);
    }

    #[test]
    fn byte_shuffles_match_the_generic_packing() {
        let source: Vec<u8> = (0..=255).collect();
        let layouts = [
            PixelLayout::ARGB32,
            PixelLayout {
                big_endian: true,
                ..PixelLayout::ARGB32
            },
            PixelLayout {
                red: Channel {
                    shift: 0,
                    mask: 0xff,
                },
                blue: Channel {
                    shift: 16,
                    mask: 0xff,
                },
                ..PixelLayout::ARGB32
            },
        ];
        for layout in layouts {
            for (format, bgra) in [
                (wgpu::TextureFormat::Rgba8Unorm, false),
                (wgpu::TextureFormat::Bgra8Unorm, true),
            ] {
                assert!(layout.byte_order(bgra).is_some());
                let mut out = Vec::new();
                layout.pack(&source, 8, 8, 32, format, &mut out).unwrap();
                let generic: Vec<u8> = source
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|pixel| layout.pack_pixel(*pixel, bgra))
                    .collect();
                assert_eq!(out, generic, "{layout:?} {format:?}");
            }
        }
    }

    #[test]
    fn bgra_into_argb32_is_a_copy() {
        assert_eq!(PixelLayout::ARGB32.byte_order(true), Some([0, 1, 2, 3]));
        assert_eq!(PixelLayout::ARGB32.byte_order(false), Some([2, 1, 0, 3]));
    }

    #[test]
    fn narrow_channels_use_the_generic_packing() {
        let layout = PixelLayout {
            red: Channel {
                shift: 11,
                mask: 0x1f,
            },
            green: Channel {
                shift: 5,
                mask: 0x3f,
            },
            blue: Channel {
                shift: 0,
                mask: 0x1f,
            },
            alpha: Channel { shift: 16, mask: 0 },
            ..PixelLayout::ARGB32
        };
        assert_eq!(layout.byte_order(false), None);
        let mut out = Vec::new();
        layout
            .pack(
                &[0xff, 0, 0xff, 0xff],
                1,
                1,
                4,
                wgpu::TextureFormat::Rgba8Unorm,
                &mut out,
            )
            .unwrap();
        assert_eq!(out, (0xf800u32 | 0x1f).to_le_bytes());
    }

    #[test]
    fn pack_rejects_a_source_too_small() {
        let mut out = Vec::new();
        assert!(
            PixelLayout::ARGB32
                .pack(&[0; 7], 2, 1, 8, wgpu::TextureFormat::Rgba8Unorm, &mut out)
                .is_err()
        );
    }
}
