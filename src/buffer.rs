use core::convert::Infallible;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget, Point, Size},
    primitives::Rectangle,
    Pixel,
};

/// A compact buffer for storing binary coloured display data.
///
/// This buffer packs the data such that each byte represents 8 pixels.
pub struct BinaryBuffer<const S: usize> {
    size: Size,
    bytes_per_row: usize,
    // Data rounds the length of each row up to the next whole byte.
    data: [u8; S],
}

pub const fn binary_buffer_length(size: Size) -> usize {
    (size.width as usize / 8) * size.height as usize
}

impl<const S: usize> BinaryBuffer<S> {
    /// Creates a new [BinaryBuffer] with all pixels set to `BinaryColor::Off`.
    pub fn new(size: Size) -> Self {
        debug_assert_eq!(
            size.width % 8,
            0,
            "Width must be a multiple of 8 for binary packing."
        );
        Self {
            bytes_per_row: size.width as usize / 8,
            size,
            data: [0; S],
        }
    }

    /// Access the packed buffer data.
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

impl<const S: usize> Dimensions for BinaryBuffer<S> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(Point::zero(), self.size)
    }
}

impl<const S: usize> DrawTarget for BinaryBuffer<S> {
    type Color = BinaryColor;

    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels.into_iter() {
            if point.x < 0
                || point.x >= self.size.width as i32
                || point.y < 0
                || point.y >= self.size.height as i32
            {
                continue; // Skip out-of-bounds pixels
            }

            let byte_index = (point.x as usize) / 8 + (point.y as usize * self.bytes_per_row);
            let bit_index = (point.x as usize) % 8;

            if color == BinaryColor::On {
                self.data[byte_index] |= 1 << bit_index;
            } else {
                self.data[byte_index] &= !(1 << bit_index);
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        let drawable_area = self.bounding_box().intersection(area);
        if drawable_area.size.width == 0 || drawable_area.size.height == 0 {
            return Ok(()); // Nothing to fill
        }
        let y_start = area.top_left.y;
        let y_end = area.top_left.y + area.size.height as i32;
        let x_start = area.top_left.x;
        let x_end = area.top_left.x + area.size.width as i32;
        let mut row_start_byte = if y_start <= 0 {
            0
        } else {
            y_start as usize * self.bytes_per_row
        };
        let (x_start_byte, x_start_bit) = if x_start <= 0 {
            (0, 0)
        } else {
            ((x_start / 8) as usize, (x_start % 8) as usize)
        };
        let mut colors_iterator = colors.into_iter();
        for y in y_start..y_end {
            let mut byte_index = x_start_byte + row_start_byte;
            let mut x_bit = x_start_bit;
            for x in x_start..x_end {
                let Some(color) = colors_iterator.next() else {
                    return Ok(()); // Stop if we run out of colors
                };
                if y < 0 || y >= self.size.height as i32 || x < 0 || x >= self.size.width as i32 {
                    continue; // Skip out-of-bounds pixels
                }
                let byte = &mut self.data[byte_index];
                match color {
                    BinaryColor::On => {
                        *byte |= 1 << x_bit;
                    }
                    BinaryColor::Off => {
                        *byte &= !(1 << x_bit);
                    }
                }
                x_bit += 1;
                if x_bit == 8 {
                    // Move to the next byte
                    x_bit = 0;
                    byte_index += 1;
                }
            }
            row_start_byte += self.size.width as usize;
        }

        Ok(())
    }

    // TODO: implement `fill_solid`
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics::pixelcolor::BinaryColor;

    #[test]
    fn test_binary_buffer_draw_iter_singles() {
        const SIZE: Size = Size::new(16, 4);
        const BUFFER_LENGTH: usize = binary_buffer_length(SIZE);
        let mut buffer = BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE);

        // Draw a pixel at the beginning.
        buffer
            .draw_iter([Pixel(Point::new(0, 0), BinaryColor::On)])
            .unwrap();
        assert_eq!(buffer.data[0], 0b1);

        // Draw a pixel in the center.
        buffer
            .draw_iter([Pixel(Point::new(10, 2), BinaryColor::On)])
            .unwrap();
        assert_eq!(buffer.data[5], 0b100);

        // Draw a pixel at the end.
        buffer
            .draw_iter([Pixel(Point::new(15, 3), BinaryColor::On)])
            .unwrap();
        assert_eq!(buffer.data[7], 0b10000000);
    }

    #[test]
    fn test_binary_buffer_draw_iter_multiple() {
        const SIZE: Size = Size::new(16, 4);
        const BUFFER_LENGTH: usize = binary_buffer_length(SIZE);
        let mut buffer = BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE);

        // Draw several pixels in a row.
        buffer
            .draw_iter([
                Pixel(Point::new(1, 0), BinaryColor::On),
                Pixel(Point::new(2, 0), BinaryColor::On),
                Pixel(Point::new(3, 0), BinaryColor::On),
                Pixel(Point::new(2, 0), BinaryColor::Off),
                Pixel(Point::new(1, 1), BinaryColor::On),
            ])
            .unwrap();

        assert_eq!(buffer.data[0], 0b00001010);
        assert_eq!(buffer.data[2], 0b00000010);
    }

    #[test]
    fn test_binary_buffer_draw_iter_out_of_bounds() {
        const SIZE: Size = Size::new(16, 4);
        const BUFFER_LENGTH: usize = binary_buffer_length(SIZE);
        let mut buffer = BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE);
        let previous_data = buffer.data;

        // Draw several pixels in a row.
        buffer
            .draw_iter([
                Pixel(Point::new(-1, 0), BinaryColor::On),
                Pixel(Point::new(0, -1), BinaryColor::On),
                Pixel(Point::new(16, 0), BinaryColor::On),
                Pixel(Point::new(0, 4), BinaryColor::On),
            ])
            .unwrap();

        assert_eq!(
            buffer.data, previous_data,
            "Data should not change when drawing out-of-bounds pixels."
        );
    }
}
