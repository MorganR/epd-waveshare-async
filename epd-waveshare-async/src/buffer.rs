use core::{
    cmp::{max, min},
    convert::Infallible,
};

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{Dimensions, DrawTarget, Point, Size},
    primitives::Rectangle,
    Pixel,
};

/// A compact buffer for storing binary coloured display data.
///
/// This buffer packs the data such that each byte represents 8 pixels.
pub struct BinaryBuffer<const L: usize> {
    size: Size,
    bytes_per_row: usize,
    // Data rounds the length of each row up to the next whole byte.
    data: [u8; L],
}

/// Computes the correct size for the binary buffer based on the given dimensions.
pub const fn binary_buffer_length(size: Size) -> usize {
    (size.width as usize / 8) * size.height as usize
}

impl<const L: usize> BinaryBuffer<L> {
    /// Creates a new [BinaryBuffer] with all pixels set to `BinaryColor::Off`.
    ///
    /// The dimensions must match the buffer length `L`, and the width must be a multiple of 8.
    ///
    /// ```
    /// use embedded_graphics::prelude::Size;
    /// use epd_waveshare_async::buffer::{binary_buffer_length, BinaryBuffer};
    ///
    /// const DIMENSIONS: Size = Size::new(8, 8);
    /// let buffer = BinaryBuffer::<{binary_buffer_length(DIMENSIONS)}>::new(DIMENSIONS);
    /// ```
    pub fn new(dimensions: Size) -> Self {
        debug_assert_eq!(
            dimensions.width % 8,
            0,
            "Width must be a multiple of 8 for binary packing."
        );
        debug_assert_eq!(
            binary_buffer_length(dimensions),
            L,
            "Size must match given dimensions"
        );
        Self {
            bytes_per_row: dimensions.width as usize / 8,
            size: dimensions,
            data: [0; L],
        }
    }

    /// Access the packed buffer data.
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

impl<const L: usize> Dimensions for BinaryBuffer<L> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(Point::zero(), self.size)
    }
}

impl<const L: usize> DrawTarget for BinaryBuffer<L> {
    type Color = BinaryColor;

    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        // Benchmarking: 60ms for checker pattern in epd2in9 sample program.
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
                self.data[byte_index] |= 0x80 >> bit_index;
            } else {
                self.data[byte_index] &= !(0x80 >> bit_index);
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        // Benchmarking: 39ms for checker pattern in epd2in9 sample program.
        let drawable_area = self.bounding_box().intersection(area);
        if drawable_area.size.width == 0 || drawable_area.size.height == 0 {
            return Ok(()); // Nothing to fill
        }

        let y_start = area.top_left.y;
        let y_end = area.top_left.y + area.size.height as i32;
        let x_start = area.top_left.x;
        let x_end = area.top_left.x + area.size.width as i32;

        let mut colors_iter = colors.into_iter();
        // TODO: Adjust indexes to be within bounds.
        let mut byte_index = max(y_start, 0) as usize * self.bytes_per_row;
        let row_start_byte_offset = max(x_start, 0) as usize / 8;
        let row_end_byte_offset =
            self.bytes_per_row - (min(x_end, self.size.width as i32) as usize / 8);
        for y in y_start..y_end {
            if y < 0 || y >= self.size.height as i32 {
                // Skip out-of-bounds rows
                for _ in x_start..x_end {
                    colors_iter.next();
                }
                continue;
            }

            byte_index += row_start_byte_offset;
            let mut bit_index = (max(x_start, 0) as usize) % 8;

            // Y is within bounds, check X.
            for x in x_start..x_end {
                if x < 0 || x >= self.size.width as i32 {
                    // Skip out-of-bounds pixels
                    colors_iter.next();
                    continue;
                }

                // Exit if there are no more colors to apply.
                let Some(color) = colors_iter.next() else {
                    return Ok(());
                };

                if color == BinaryColor::On {
                    self.data[byte_index] |= 0x80 >> bit_index;
                } else {
                    self.data[byte_index] &= !(0x80 >> bit_index);
                }

                bit_index += 1;
                if bit_index == 8 {
                    // Move to the next byte after every 8 pixels
                    byte_index += 1;
                    bit_index = 0;
                }
            }

            byte_index += row_end_byte_offset;
        }

        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        // Benchmarking: 3ms for checker pattern in epd2in9 sample program.
        let drawable_area = self.bounding_box().intersection(area);
        if drawable_area.size.width == 0 || drawable_area.size.height == 0 {
            return Ok(()); // Nothing to fill
        }

        let y_start = drawable_area.top_left.y;
        let y_end = drawable_area.top_left.y + drawable_area.size.height as i32;
        let x_start = drawable_area.top_left.x;
        let x_end = drawable_area.top_left.x + drawable_area.size.width as i32;

        let x_full_bytes_start = min(x_start + x_start % 8, x_end);
        let x_full_bytes_end = max(x_end - (x_end % 8), x_start);
        let num_full_bytes_per_row = (x_full_bytes_end - x_full_bytes_start) / 8;

        let mut byte_index = y_start as usize * self.bytes_per_row;
        let row_start_byte_offset = x_start as usize / 8;
        let row_end_byte_offset = self.bytes_per_row - (x_end as usize / 8);
        for _y in y_start..y_end {
            byte_index += row_start_byte_offset;
            let mut bit_index = (x_start as usize) % 8;

            macro_rules! set_next_bit {
                () => {
                    if color == BinaryColor::On {
                        self.data[byte_index] |= 0x80 >> bit_index;
                    } else {
                        self.data[byte_index] &= !(0x80 >> bit_index);
                    }
                    bit_index += 1;
                    if bit_index == 8 {
                        // Move to the next byte after every 8 pixels
                        byte_index += 1;
                        bit_index = 0;
                    }
                };
            }

            if num_full_bytes_per_row == 0 {
                for _x in x_start..x_end {
                    set_next_bit!();
                }
            } else {
                for _x in x_start..x_full_bytes_start {
                    set_next_bit!();
                }

                // Fast fill for any fully covered bytes in the row.
                for _ in 0..num_full_bytes_per_row {
                    if color == BinaryColor::On {
                        self.data[byte_index] = 0xFF;
                    } else {
                        self.data[byte_index] = 0x00;
                    }
                    byte_index += 1;
                }

                // Set the partially covered byte at the end of the row, if any.
                bit_index = x_full_bytes_end as usize % 8;
                for _x in x_full_bytes_end..x_end {
                    set_next_bit!();
                }
            }

            byte_index += row_end_byte_offset;
        }

        Ok(())
    }
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
        assert_eq!(buffer.data[0], 0b10000000);

        // Draw a pixel in the center.
        buffer
            .draw_iter([Pixel(Point::new(10, 2), BinaryColor::On)])
            .unwrap();
        assert_eq!(buffer.data[5], 0b00100000);

        // Draw a pixel at the end.
        buffer
            .draw_iter([Pixel(Point::new(15, 3), BinaryColor::On)])
            .unwrap();
        assert_eq!(buffer.data[7], 0b1);
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

        assert_eq!(buffer.data[0], 0b01010000);
        assert_eq!(buffer.data[2], 0b01000000);
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

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic]
    fn test_binary_buffer_must_have_aligned_width() {
        let _ = BinaryBuffer::<16>::new(Size::new(10, 10));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic]
    fn test_binary_buffer_size_must_match_dimensions() {
        let _ = BinaryBuffer::<16>::new(Size::new(16, 10));
    }

    #[test]
    fn test_binary_buffer_fill_continguous() {
        // 8 rows, 1 byte each.
        const SIZE: Size = Size::new(24, 8);
        const BUFFER_LENGTH: usize = binary_buffer_length(SIZE);
        let mut buffer = BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE);

        // Draw diagonal squares.
        buffer
            .fill_contiguous(
                &Rectangle::new(Point::new(-4, -4), Size::new(8, 8)),
                [BinaryColor::On; 8 * 8],
            )
            .unwrap();
        buffer
            .fill_contiguous(
                // Go out of bounds to ensure it doesn't panic.
                &Rectangle::new(Point::new(6, 2), Size::new(12, 4)),
                [BinaryColor::On; 12 * 4],
            )
            .unwrap();
        buffer
            .fill_contiguous(
                // Go out of bounds to ensure it doesn't panic.
                &Rectangle::new(Point::new(20, 4), Size::new(8, 8)),
                [BinaryColor::On; 8 * 8],
            )
            .unwrap();

        #[rustfmt::skip]
        let expected: [u8; 3 * 8] = [
            0b11110000, 0b00000000, 0b00000000,
            0b11110000, 0b00000000, 0b00000000,
            0b11110011, 0b11111111, 0b11000000,
            0b11110011, 0b11111111, 0b11000000,
            0b00000011, 0b11111111, 0b11001111,
            0b00000011, 0b11111111, 0b11001111,
            0b00000000, 0b00000000, 0b00001111,
            0b00000000, 0b00000000, 0b00001111,
        ];
        assert_eq!(buffer.data(), &expected);
    }

    #[test]
    fn test_binary_buffer_fill_solid() {
        // 8 rows, 1 byte each.
        const SIZE: Size = Size::new(24, 8);
        const BUFFER_LENGTH: usize = binary_buffer_length(SIZE);
        let mut buffer = BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE);

        // Draw diagonal squares.
        buffer
            .fill_solid(
                &Rectangle::new(Point::new(-4, -4), Size::new(8, 8)),
                BinaryColor::On,
            )
            .unwrap();
        buffer
            .fill_solid(
                // Go out of bounds to ensure it doesn't panic.
                &Rectangle::new(Point::new(6, 2), Size::new(12, 4)),
                BinaryColor::On,
            )
            .unwrap();
        buffer
            .fill_solid(
                // Go out of bounds to ensure it doesn't panic.
                &Rectangle::new(Point::new(20, 4), Size::new(8, 8)),
                BinaryColor::On,
            )
            .unwrap();

        #[rustfmt::skip]
        let expected: [u8; 3 * 8] = [
            0b11110000, 0b00000000, 0b00000000,
            0b11110000, 0b00000000, 0b00000000,
            0b11110011, 0b11111111, 0b11000000,
            0b11110011, 0b11111111, 0b11000000,
            0b00000011, 0b11111111, 0b11001111,
            0b00000011, 0b11111111, 0b11001111,
            0b00000000, 0b00000000, 0b00001111,
            0b00000000, 0b00000000, 0b00001111,
        ];
        assert_eq!(buffer.data(), &expected);
    }
}
