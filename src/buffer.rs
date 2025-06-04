use core::convert::Infallible;

use embedded_graphics::{pixelcolor::BinaryColor, prelude::{Dimensions, DrawTarget, Point, Size}, primitives::Rectangle, Pixel};

/// A compact buffer for storing binary coloured display data. 
/// 
/// This buffer packs the data such that each byte represents 8 pixels. 
pub struct BinaryBuffer<const SW: usize, const BW: usize, const H: usize> {
    // Data rounds the length of each row up to the next whole byte.
    rows: [[u8; BW]; H],
}

impl <const SW: usize, const BW: usize, const H: usize> BinaryBuffer<SW, BW, H> {
    /// Computes the buffer width in bytes to use for the given screen width in pixels.
    /// 
    /// This can be used to set the `BW` constant when creating a [BinaryBuffer].
    pub const fn buffer_width_for_screen_width<const W: usize>() -> usize {
        (W + 7) / 8 // Round up to the next byte
    }

    /// Creates a new [BinaryBuffer] with all pixels set to `BinaryColor::Off`.
    pub fn new() -> Self {
        Self {
            rows: [[0; BW]; H],
        }
    }
}

impl <const SW: usize, const BW: usize, const H: usize> Dimensions for BinaryBuffer<SW, BW, H> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(
            Point::new(0, 0),
            Size::new(SW as u32, H as u32),
        )
    }
}

impl <const SW: usize, const BW: usize, const H: usize> DrawTarget for BinaryBuffer<SW, BW, H> {
    type Color = BinaryColor;

    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>> {
        for Pixel(point, color) in pixels.into_iter() {
            if point.x < 0 || point.x >= SW as i32 || point.y < 0 || point.y >= H as i32 {
                continue; // Skip out-of-bounds pixels
            }

            let byte_index = (point.x as usize) / 8;
            let bit_index = (point.x as usize) % 8;

            if color == BinaryColor::On {
                self.rows[point.y as usize][byte_index] |= 1 << bit_index;
            } else {
                self.rows[point.y as usize][byte_index] &= !(1 << bit_index);
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Self::Color>, {
        let drawable_area = self.bounding_box().intersection(area);
        if drawable_area.size.width == 0 || drawable_area.size.height == 0  {
            return Ok(()); // Nothing to fill
        }
        let y_start = area.top_left.y;
        let y_end = area.top_left.y + area.size.height as i32;
        let x_start = area.top_left.x;
        let x_end = area.top_left.x + area.size.width as i32;
        let (x_start_byte, x_start_bit) = if x_start <= 0 {
            (0, 0)
        } else {
            ((x_start / 8) as usize, (x_start % 8) as usize)
        };
        let mut colors_iterator = colors.into_iter();
        for y in y_start..y_end {
            let mut x_byte = x_start_byte;
            let mut x_bit = x_start_bit;
            for x in x_start..x_end {
                let Some(color) = colors_iterator.next() else {
                    return Ok(()); // Stop if we run out of colors
                };
                if y < 0 || y >= H as i32 || x < 0 || x >= BW as i32 {
                    continue; // Skip out-of-bounds pixels
                }
                let byte = &mut self.rows[y as usize][x_byte];
                match color {
                    BinaryColor::On => {
                        *byte |= 1 << x_bit;
                    },
                    BinaryColor::Off => {
                        *byte &= !(1 << x_bit);
                    },
                }
                x_bit += 1;
                if x_bit == 8 {
                    // Move to the next byte
                    x_bit = 0;
                    x_byte += 1;
                }
            }
        }

        Ok(())
    }
}

