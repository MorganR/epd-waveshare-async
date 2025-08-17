use core::{
    cmp::{max, min},
    convert::Infallible,
};

use embedded_graphics::{
    pixelcolor::{BinaryColor, Gray2},
    prelude::{Dimensions, DrawTarget, GrayColor, Point, Size},
    primitives::Rectangle,
    Pixel,
};
use heapless::Vec;

/// Provides a view into a display buffer's data. This buffer is encoded into a set number of frames and bits per pixel.
pub trait BufferView<const BITS: usize, const FRAMES: usize> {
    /// Returns the display window covered by this buffer.
    fn window(&self) -> Rectangle;

    /// Returns the data to be written to this window.
    fn data(&self) -> [&[u8]; FRAMES];
}

/// A compact buffer for storing binary coloured display data.
///
/// This buffer packs the data such that each byte represents 8 pixels.
#[derive(Clone)]
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

impl<const L: usize> BufferView<1, 1> for BinaryBuffer<L> {
    fn window(&self) -> Rectangle {
        Rectangle::new(Point::zero(), self.size)
    }

    fn data(&self) -> [&[u8]; 1] {
        [self.data()]
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
        {
            let drawable_area = self.bounding_box().intersection(area);
            if drawable_area.size.width == 0 || drawable_area.size.height == 0 {
                return Ok(()); // Nothing to fill
            }
        }

        let y_start = area.top_left.y;
        let y_end = area.top_left.y + area.size.height as i32;
        let x_start = area.top_left.x;
        let x_end = area.top_left.x + area.size.width as i32;

        let mut colors_iter = colors.into_iter();
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

        // Full bytes start at next multiple of 8 from x_start (inclusive if x_start is multiple)
        let x_full_bytes_start = min(((x_start + 7) / 8) * 8, x_end);
        // Full bytes end at last multiple of 8 before x_end (inclusive if x_end is multiple)
        let x_full_bytes_end = max(x_end / 8 * 8, x_start);
        let num_full_bytes_per_row = (x_full_bytes_end - x_full_bytes_start) / 8;

        let mut byte_index = y_start as usize * self.bytes_per_row;
        let row_start_byte_offset = x_start as usize / 8;
        let row_end_byte_offset = self.bytes_per_row - (x_end as usize / 8);
        for _y in y_start..y_end {
            byte_index += row_start_byte_offset;
            let mut bit_index = (x_start as usize) % 8;

            /// Sets the next bit from `color` and advances `bit_index` and `byte_index`
            /// appropriately.
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
                // There are no full bytes in this row, so just set colors bitwise.
                for _x in x_start..x_end {
                    set_next_bit!();
                }
            } else {
                // Set colors bitwise in the first byte if it's not byte-aligned.
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

/// A buffer supporting 2-bit grayscale colours. This buffer splits the 2 bits into two separate single-bit framebuffers.
#[derive(Clone)]
pub struct Gray2SplitBuffer<const L: usize> {
    pub low: BinaryBuffer<L>,
    pub high: BinaryBuffer<L>,
}

/// Computes the correct size for the [Gray2SplitBuffer] based on the given dimensions.
pub const fn gray2_split_buffer_length(size: Size) -> usize {
    binary_buffer_length(size)
}

impl<const L: usize> Gray2SplitBuffer<L> {
    /// Creates a new [Gray2SplitBuffer] with all pixels set to 0.
    ///
    /// The dimensions must match the buffer length `L`, and the width must be a multiple of 8.
    ///
    /// ```
    /// use embedded_graphics::prelude::Size;
    /// use epd_waveshare_async::buffer::{gray2_split_buffer_length, Gray2SplitBuffer};
    ///
    /// const DIMENSIONS: Size = Size::new(8, 8);
    /// let buffer = Gray2SplitBuffer::<{gray2_split_buffer_length(DIMENSIONS)}>::new(DIMENSIONS);
    /// ```
    pub fn new(dimensions: Size) -> Self {
        Self {
            low: BinaryBuffer::new(dimensions),
            high: BinaryBuffer::new(dimensions),
        }
    }
}

impl<const L: usize> BufferView<1, 2> for Gray2SplitBuffer<L> {
    fn window(&self) -> Rectangle {
        Rectangle::new(Point::zero(), self.low.size)
    }

    fn data(&self) -> [&[u8]; 2] {
        [self.low.data(), self.high.data()]
    }
}

impl<const L: usize> Dimensions for Gray2SplitBuffer<L> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(Point::zero(), self.low.size)
    }
}

fn to_low_and_high_as_binary(g: Gray2) -> (BinaryColor, BinaryColor) {
    let luma = g.luma();
    let low = if (luma & 1) == 0 {
        BinaryColor::Off
    } else {
        BinaryColor::On
    };
    let high = if (luma & 0b10) == 0 {
        BinaryColor::Off
    } else {
        BinaryColor::On
    };
    (low, high)
}

const GRAY_ITER_CHUNK_SIZE: usize = 128;

impl<const L: usize> DrawTarget for Gray2SplitBuffer<L> {
    type Color = Gray2;

    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        // We iterate the data into chunks because:
        // 1. It's usually less memory pressure than creating two more full-size vectors.
        // 2. The iterator is allowed to go out-of-bounds, so it might actually be longer than L.
        let mut low_chunk: Vec<Pixel<BinaryColor>, GRAY_ITER_CHUNK_SIZE> = Vec::new();
        let mut high_chunk: Vec<Pixel<BinaryColor>, GRAY_ITER_CHUNK_SIZE> = Vec::new();
        for p in pixels.into_iter() {
            let (low, high) = to_low_and_high_as_binary(p.1);
            if low_chunk.is_full() {
                self.low.draw_iter(low_chunk)?;
                low_chunk = Vec::new();
                self.high.draw_iter(high_chunk)?;
                high_chunk = Vec::new();
            }
            unsafe {
                low_chunk.push_unchecked(Pixel(p.0, low));
                high_chunk.push_unchecked(Pixel(p.0, high));
            }
        }
        if !low_chunk.is_empty() {
            self.low.draw_iter(low_chunk)?;
            self.high.draw_iter(high_chunk)?;
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        let (low, high) = to_low_and_high_as_binary(color);
        self.low.fill_solid(area, low)?;
        self.high.fill_solid(area, high)?;
        Ok(())
    }
}

pub trait Rotation {
    /// Returns the inverse rotation that reverses this rotation's effect.
    fn inverse(&self) -> Self;

    /// Rotates the given size according to this rotation type.
    fn rotate_size(&self, size: Size) -> Size;

    /// Rotates a point according to this rotation type, within overall source bounds of the given size.
    ///
    /// For example, if the given `point` is (1,2) from a 10x20 space, then [Rotate::Degrees90] would
    /// return (17, 1) in a 20x10 space. `bounds` should be the source dimensions of 10x20.
    ///
    /// ```rust
    /// # use embedded_graphics::prelude::{Point, Size};
    /// # use epd_waveshare_async::buffer::{Rotate, Rotation};
    ///
    /// let r = Rotate::Degrees90;
    /// assert_eq!(r.rotate_point(Point::new(1, 2), Size::new(10, 20)), Point::new(17, 1));
    /// ```
    fn rotate_point(&self, point: Point, bounds: Size) -> Point;

    /// Rotates a rectangle according to this rotation type, within overall source bounds of the given size.
    fn rotate_rectangle(&self, rectangle: Rectangle, bounds: Size) -> Rectangle;
}

/// Represents a 90, 180, or 270 degree clockwise rotation of a point within a given size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotate {
    Degrees90,
    Degrees180,
    Degrees270,
}

impl Rotation for Rotate {
    fn inverse(&self) -> Self {
        match self {
            Rotate::Degrees90 => Rotate::Degrees270,
            Rotate::Degrees180 => Rotate::Degrees180,
            Rotate::Degrees270 => Rotate::Degrees90,
        }
    }

    fn rotate_size(&self, size: Size) -> Size {
        match self {
            Rotate::Degrees90 | Rotate::Degrees270 => Size::new(size.height, size.width),
            Rotate::Degrees180 => size,
        }
    }

    fn rotate_point(&self, point: Point, source_bounds: Size) -> Point {
        match self {
            Rotate::Degrees90 => Point::new(source_bounds.height as i32 - point.y - 1, point.x),
            Rotate::Degrees180 => Point::new(
                source_bounds.width as i32 - point.x - 1,
                source_bounds.height as i32 - point.y - 1,
            ),
            Rotate::Degrees270 => Point::new(point.y, source_bounds.width as i32 - point.x - 1),
        }
    }

    fn rotate_rectangle(&self, rectangle: Rectangle, source_bounds: Size) -> Rectangle {
        match self {
            Rotate::Degrees90 => {
                let old_bottom_left =
                    rectangle.top_left + Point::new(0, rectangle.size.height as i32 - 1);
                let new_top_left = self.rotate_point(old_bottom_left, source_bounds);
                Rectangle::new(new_top_left, self.rotate_size(rectangle.size))
            }
            Rotate::Degrees180 => {
                let old_bottom_right = rectangle.top_left + rectangle.size - Point::new(1, 1);
                let new_top_left = self.rotate_point(old_bottom_right, source_bounds);
                Rectangle::new(new_top_left, self.rotate_size(rectangle.size))
            }
            Rotate::Degrees270 => {
                let old_top_right =
                    rectangle.top_left + Point::new(rectangle.size.width as i32 - 1, 0);
                let new_top_left = self.rotate_point(old_top_right, source_bounds);
                Rectangle::new(new_top_left, self.rotate_size(rectangle.size))
            }
        }
    }
}

/// Enables arbitrarily rotating an underlying [DrawTarget] buffer. This is useful if the default display
/// orientation does not match the desired orientation of the content.
///
/// ```text
/// let mut default_buffer = epd.new_buffer();
/// // If the default buffer is portrait, this would rotate it so you can draw to it as if it's in landscape mode.
/// let rotated_buffer = RotatedBuffer::new(&mut default_buffer, Rotate::Degrees90);
///
/// // ... Use the buffer here
///
/// epd.display_buffer(&mut spi, rotated_buffer.inner()).await?;
/// ```
pub struct RotatedBuffer<B: DrawTarget, R: Rotation> {
    bounds: Rectangle,
    buffer: B,
    rotation: R,
}

impl<B: DrawTarget, R: Rotation> RotatedBuffer<B, R> {
    pub fn new(buffer: B, rotation: R) -> Self {
        let inverse_rotation = rotation.inverse();
        let inner_bounds = buffer.bounding_box();
        let bounds = inverse_rotation.rotate_rectangle(inner_bounds, inner_bounds.size);
        Self {
            bounds,
            buffer,
            rotation,
        }
    }

    /// Provides read-only access to the inner buffer.
    pub fn inner(&mut self) -> &B {
        &self.buffer
    }

    /// Drops this rotated buffer wrapper and takes out the inner buffer.
    pub fn take_inner(self) -> B {
        self.buffer
    }
}

impl<B: DrawTarget, R: Rotation> Dimensions for RotatedBuffer<B, R> {
    fn bounding_box(&self) -> Rectangle {
        self.bounds
    }
}

impl<B: DrawTarget, R: Rotation> DrawTarget for RotatedBuffer<B, R> {
    type Color = B::Color;
    type Error = B::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let rotated_pixels = pixels.into_iter().map(|Pixel(point, color)| {
            let rotated_point = self.rotation.rotate_point(point, self.bounds.size);
            Pixel(rotated_point, color)
        });
        self.buffer.draw_iter(rotated_pixels)
    }
}

#[inline(always)]
/// Splits a 16-bit value into the two 8-bit values representing the low and high bytes.
pub(crate) fn split_low_and_high(value: u16) -> (u8, u8) {
    let low = (value & 0xFF) as u8;
    let high = ((value >> 8) & 0xFF) as u8;
    (low, high)
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

    // Test for a bug where fill_solid would skip filling bytes on some width settings
    #[test]
    fn test_binary_buffer_fill_solid_offset() {
        // 8 rows, 1 byte each.
        const SIZE: Size = Size::new(24, 8);
        const BUFFER_LENGTH: usize = binary_buffer_length(SIZE);
        let mut buffer = BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE);

        // Draw diagonal squares.
        buffer
            .fill_solid(
                // Fill a rectangle with a width of 18
                &Rectangle::new(Point::new(6, 2), Size::new(18, 3)),
                BinaryColor::On,
            )
            .unwrap();

        #[rustfmt::skip]
        let expected: [u8; 3 * 8] = [
            0b00000000, 0b00000000, 0b00000000,
            0b00000000, 0b00000000, 0b00000000,
            0b00000011, 0b11111111, 0b11111111,
            0b00000011, 0b11111111, 0b11111111,
            0b00000011, 0b11111111, 0b11111111,
            0b00000000, 0b00000000, 0b00000000,
            0b00000000, 0b00000000, 0b00000000,
            0b00000000, 0b00000000, 0b00000000,
        ];
        assert_eq!(buffer.data(), &expected);
    }

    #[test]
    fn test_gray2_split_buffer_draw_iter_singles() {
        const SIZE: Size = Size::new(16, 4);
        const BUFFER_LENGTH: usize = gray2_split_buffer_length(SIZE);
        let mut buffer = Gray2SplitBuffer::<{ BUFFER_LENGTH }>::new(SIZE);

        // Draw a pixel at the beginning.
        buffer
            .draw_iter([Pixel(Point::new(0, 0), Gray2::new(0b11))])
            .unwrap();
        assert_eq!(buffer.low.data[0], 0b10000000);
        assert_eq!(buffer.high.data[0], 0b10000000);

        // Draw a pixel in the center.
        buffer
            .draw_iter([Pixel(Point::new(10, 2), Gray2::new(0b10))])
            .unwrap();
        assert_eq!(buffer.data()[0][5], 0b00000000);
        assert_eq!(buffer.data()[1][5], 0b00100000);

        // Draw a pixel at the end.
        buffer
            .draw_iter([Pixel(Point::new(15, 3), Gray2::new(0b01))])
            .unwrap();
        assert_eq!(buffer.low.data[7], 0b1);
        assert_eq!(buffer.high.data[7], 0b0);
    }

    #[test]
    fn test_gray2_buffer_draw_iter_multiple() {
        const SIZE: Size = Size::new(16, 4);
        const BUFFER_LENGTH: usize = gray2_split_buffer_length(SIZE);
        let mut buffer = Gray2SplitBuffer::<{ BUFFER_LENGTH }>::new(SIZE);

        // Draw several pixels in a row.
        buffer
            .draw_iter([
                Pixel(Point::new(1, 0), Gray2::new(0b11)),
                Pixel(Point::new(2, 0), Gray2::new(0b11)),
                Pixel(Point::new(3, 0), Gray2::new(0b01)),
                Pixel(Point::new(2, 0), Gray2::new(0)),
                Pixel(Point::new(1, 1), Gray2::new(0b10)),
            ])
            .unwrap();

        assert_eq!(buffer.low.data[0], 0b01010000);
        assert_eq!(buffer.high.data[0], 0b01000000);
        assert_eq!(buffer.low.data[2], 0b00000000);
        assert_eq!(buffer.high.data[2], 0b01000000);
    }

    #[test]
    fn test_gray2_buffer_draw_iter_out_of_bounds() {
        const SIZE: Size = Size::new(16, 4);
        const BUFFER_LENGTH: usize = gray2_split_buffer_length(SIZE);
        let mut buffer = Gray2SplitBuffer::<{ BUFFER_LENGTH }>::new(SIZE);
        let previous = buffer.clone();

        // Draw several pixels in a row.
        buffer
            .draw_iter([
                Pixel(Point::new(-1, 0), Gray2::new(0b11)),
                Pixel(Point::new(0, -1), Gray2::new(0b11)),
                Pixel(Point::new(16, 0), Gray2::new(0b11)),
                Pixel(Point::new(0, 4), Gray2::new(0b11)),
            ])
            .unwrap();

        assert_eq!(
            buffer.data(),
            previous.data(),
            "Data should not change when drawing out-of-bounds pixels."
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic]
    fn test_gray2_buffer_must_have_aligned_width() {
        let _ = Gray2SplitBuffer::<16>::new(Size::new(10, 10));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic]
    fn test_gray2_buffer_size_must_match_dimensions() {
        let _ = Gray2SplitBuffer::<16>::new(Size::new(16, 10));
    }

    #[test]
    fn test_gray2_buffer_fill_solid() {
        // 8 rows, 1 byte each.
        const SIZE: Size = Size::new(24, 8);
        const BUFFER_LENGTH: usize = gray2_split_buffer_length(SIZE);
        let mut buffer = Gray2SplitBuffer::<{ BUFFER_LENGTH }>::new(SIZE);

        // Draw diagonal squares.
        buffer
            .fill_solid(
                &Rectangle::new(Point::new(-4, -4), Size::new(8, 8)),
                Gray2::new(0b11),
            )
            .unwrap();
        buffer
            .fill_solid(
                // Go out of bounds to ensure it doesn't panic.
                &Rectangle::new(Point::new(6, 2), Size::new(12, 4)),
                Gray2::new(0b10),
            )
            .unwrap();
        buffer
            .fill_solid(
                // Go out of bounds to ensure it doesn't panic.
                &Rectangle::new(Point::new(20, 4), Size::new(8, 8)),
                Gray2::new(0b01),
            )
            .unwrap();

        #[rustfmt::skip]
        let expected_low: [u8; 3 * 8] = [
            0b11110000, 0b00000000, 0b00000000,
            0b11110000, 0b00000000, 0b00000000,
            0b11110000, 0b00000000, 0b00000000,
            0b11110000, 0b00000000, 0b00000000,
            0b00000000, 0b00000000, 0b00001111,
            0b00000000, 0b00000000, 0b00001111,
            0b00000000, 0b00000000, 0b00001111,
            0b00000000, 0b00000000, 0b00001111,
        ];
        #[rustfmt::skip]
        let expected_high: [u8; 3 * 8] = [
            0b11110000, 0b00000000, 0b00000000,
            0b11110000, 0b00000000, 0b00000000,
            0b11110011, 0b11111111, 0b11000000,
            0b11110011, 0b11111111, 0b11000000,
            0b00000011, 0b11111111, 0b11000000,
            0b00000011, 0b11111111, 0b11000000,
            0b00000000, 0b00000000, 0b00000000,
            0b00000000, 0b00000000, 0b00000000,
        ];
        assert_eq!(buffer.data()[0], &expected_low);
        assert_eq!(buffer.data()[1], &expected_high);
    }

    #[test]
    fn test_rotated_buffer_bounds() {
        const SIZE: Size = Size::new(8, 24);
        const BUFFER_LENGTH: usize = binary_buffer_length(SIZE);

        let mut rotated_buffer = RotatedBuffer::new(
            BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE),
            Rotate::Degrees90,
        );
        assert_eq!(
            rotated_buffer.bounding_box(),
            Rectangle::new(Point::new(0, 0), Size::new(24, 8))
        );

        rotated_buffer = RotatedBuffer::new(
            BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE),
            Rotate::Degrees180,
        );
        assert_eq!(
            rotated_buffer.bounding_box(),
            Rectangle::new(Point::new(0, 0), Size::new(8, 24))
        );

        rotated_buffer = RotatedBuffer::new(
            BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE),
            Rotate::Degrees270,
        );
        assert_eq!(
            rotated_buffer.bounding_box(),
            Rectangle::new(Point::new(0, 0), Size::new(24, 8))
        );
    }

    #[test]
    fn test_rotated_buffer_draw_iter() {
        const SIZE: Size = Size::new(8, 4);
        const BUFFER_LENGTH: usize = binary_buffer_length(SIZE);

        let mut rotated_buffer = RotatedBuffer::new(
            BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE),
            Rotate::Degrees90,
        );
        rotated_buffer
            .draw_iter([
                Pixel(Point::new(-1, -1), BinaryColor::On), // Should be ignored.
                Pixel(Point::new(0, 0), BinaryColor::On),
                Pixel(Point::new(1, 1), BinaryColor::On),
                Pixel(Point::new(2, 2), BinaryColor::On),
            ])
            .unwrap();
        #[rustfmt::skip]
        let expected: [u8; 4] = [
                0b00000001,
                0b00000010,
                0b00000100,
                0b00000000,
            ];
        assert_eq!(rotated_buffer.inner().data(), &expected);

        rotated_buffer = RotatedBuffer::new(
            BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE),
            Rotate::Degrees180,
        );
        rotated_buffer
            .draw_iter([
                Pixel(Point::new(-1, -1), BinaryColor::On), // Should be ignored.
                Pixel(Point::new(0, 0), BinaryColor::On),
                Pixel(Point::new(1, 1), BinaryColor::On),
                Pixel(Point::new(2, 2), BinaryColor::On),
            ])
            .unwrap();
        #[rustfmt::skip]
        let expected: [u8; 4] = [
                0b00000000,
                0b00000100,
                0b00000010,
                0b00000001,
            ];
        assert_eq!(rotated_buffer.inner().data(), &expected);

        rotated_buffer = RotatedBuffer::new(
            BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE),
            Rotate::Degrees270,
        );
        rotated_buffer
            .draw_iter([
                Pixel(Point::new(-1, -1), BinaryColor::On), // Should be ignored.
                Pixel(Point::new(0, 0), BinaryColor::On),
                Pixel(Point::new(1, 1), BinaryColor::On),
                Pixel(Point::new(2, 2), BinaryColor::On),
            ])
            .unwrap();
        #[rustfmt::skip]
        let expected: [u8; 4] = [
                0b00000000,
                0b00100000,
                0b01000000,
                0b10000000,
            ];
        assert_eq!(rotated_buffer.inner().data(), &expected);
    }

    #[test]
    fn test_rotated_buffer_fill_contiguous() {
        // 8 rows, 1 byte each.
        const SIZE: Size = Size::new(8, 6);
        const BUFFER_LENGTH: usize = binary_buffer_length(SIZE);
        let buffer = BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE);

        let mut rotated_buffer = RotatedBuffer::new(buffer.clone(), Rotate::Degrees90);
        rotated_buffer
            .fill_contiguous(
                &Rectangle::new(Point::new(-4, -4), Size::new(8, 8)),
                [BinaryColor::On; 8 * 8],
            )
            .unwrap();

        #[rustfmt::skip]
        let expected: [u8; 6] = [
            0b00001111,
            0b00001111,
            0b00001111,
            0b00001111,
            0b00000000,
            0b00000000,
        ];
        assert_eq!(rotated_buffer.inner().data(), &expected);

        rotated_buffer = RotatedBuffer::new(buffer.clone(), Rotate::Degrees180);
        rotated_buffer
            .fill_contiguous(
                &Rectangle::new(Point::new(-4, -4), Size::new(8, 8)),
                [BinaryColor::On; 8 * 8],
            )
            .unwrap();

        #[rustfmt::skip]
        let expected: [u8; 6] = [
            0b00000000,
            0b00000000,
            0b00001111,
            0b00001111,
            0b00001111,
            0b00001111,
        ];
        assert_eq!(rotated_buffer.inner().data(), &expected);

        rotated_buffer = RotatedBuffer::new(buffer.clone(), Rotate::Degrees270);
        rotated_buffer
            .fill_contiguous(
                &Rectangle::new(Point::new(-4, -4), Size::new(8, 8)),
                [BinaryColor::On; 8 * 8],
            )
            .unwrap();

        #[rustfmt::skip]
        let expected: [u8; 6] = [
            0b00000000,
            0b00000000,
            0b11110000,
            0b11110000,
            0b11110000,
            0b11110000,
        ];
        assert_eq!(rotated_buffer.inner().data(), &expected);
    }

    #[test]
    fn test_rotated_buffer_fill_solid() {
        // 8 rows, 1 byte each.
        const SIZE: Size = Size::new(8, 6);
        const BUFFER_LENGTH: usize = binary_buffer_length(SIZE);
        let buffer = BinaryBuffer::<{ BUFFER_LENGTH }>::new(SIZE);

        let mut rotated_buffer = RotatedBuffer::new(buffer.clone(), Rotate::Degrees90);
        rotated_buffer
            .fill_solid(
                &Rectangle::new(Point::new(-4, -4), Size::new(8, 8)),
                BinaryColor::On,
            )
            .unwrap();

        #[rustfmt::skip]
        let expected: [u8; 6] = [
            0b00001111,
            0b00001111,
            0b00001111,
            0b00001111,
            0b00000000,
            0b00000000,
        ];
        assert_eq!(rotated_buffer.inner().data(), &expected);

        rotated_buffer = RotatedBuffer::new(buffer.clone(), Rotate::Degrees180);
        rotated_buffer
            .fill_solid(
                &Rectangle::new(Point::new(-4, -4), Size::new(8, 8)),
                BinaryColor::On,
            )
            .unwrap();

        #[rustfmt::skip]
        let expected: [u8; 6] = [
            0b00000000,
            0b00000000,
            0b00001111,
            0b00001111,
            0b00001111,
            0b00001111,
        ];
        assert_eq!(rotated_buffer.inner().data(), &expected);

        rotated_buffer = RotatedBuffer::new(buffer.clone(), Rotate::Degrees270);
        rotated_buffer
            .fill_solid(
                &Rectangle::new(Point::new(-4, -4), Size::new(8, 8)),
                BinaryColor::On,
            )
            .unwrap();

        #[rustfmt::skip]
        let expected: [u8; 6] = [
            0b00000000,
            0b00000000,
            0b11110000,
            0b11110000,
            0b11110000,
            0b11110000,
        ];
        assert_eq!(rotated_buffer.inner().data(), &expected);
    }

    #[test]
    fn test_rotate_near_corner() {
        let mut r = Rotate::Degrees90;
        // (1,1) in [10, 20] becomes (18, 1) in [20, 10].
        assert_eq!(
            Point::new(18, 1),
            r.rotate_point(Point::new(1, 1), Size::new(10, 20))
        );
        r = Rotate::Degrees180;
        // (1,1) in [10, 20] becomes (8, 18) in [10, 20].
        assert_eq!(
            Point::new(8, 18),
            r.rotate_point(Point::new(1, 1), Size::new(10, 20))
        );
        r = Rotate::Degrees270;
        // (1,1) in [10, 20] becomes (1, 8) in [20, 10].
        assert_eq!(
            Point::new(1, 8),
            r.rotate_point(Point::new(1, 1), Size::new(10, 20))
        );
    }

    #[test]
    fn test_rotate_centre() {
        let mut r = Rotate::Degrees90;
        assert_eq!(
            Point::new(2, 2),
            r.rotate_point(Point::new(2, 2), Size::new(5, 5))
        );
        r = Rotate::Degrees180;
        assert_eq!(
            Point::new(2, 2),
            r.rotate_point(Point::new(2, 2), Size::new(5, 5))
        );
        r = Rotate::Degrees270;
        assert_eq!(
            Point::new(2, 2),
            r.rotate_point(Point::new(2, 2), Size::new(5, 5))
        );
    }

    #[test]
    fn test_rotate_size() {
        let mut r = Rotate::Degrees90;
        assert_eq!(Size::new(5, 10), r.rotate_size(Size::new(10, 5)));
        r = Rotate::Degrees180;
        assert_eq!(Size::new(10, 5), r.rotate_size(Size::new(10, 5)));
        r = Rotate::Degrees270;
        assert_eq!(Size::new(5, 10), r.rotate_size(Size::new(10, 5)));
    }

    #[test]
    fn test_rotate_rectangle() {
        let mut r = Rotate::Degrees90;
        let rect = Rectangle::new(Point::new(1, 1), Size::new(3, 2));
        // Assume we're rotating _into_ an 8x4 destination buffer.
        let _dest_bounds = Size::new(8, 4);
        let mut source_bounds = Size::new(4, 8);
        let rotated = r.rotate_rectangle(rect, source_bounds);
        // (1, 1) in [4, 8] becomes (6, 1) in [8, 4].
        // The old bottom left is (1, 2), which becomes (5, 1).
        assert_eq!(rotated.top_left, Point::new(5, 1));
        assert_eq!(rotated.size, Size::new(2, 3));

        r = Rotate::Degrees180;
        source_bounds = Size::new(8, 4);
        let rotated = r.rotate_rectangle(rect, source_bounds);
        // (1, 1) in [8, 4] becomes (6, 2) in [8, 4].
        // The old bottom right is (3, 2), which becomes (4, 1).
        assert_eq!(rotated.top_left, Point::new(4, 1));
        assert_eq!(rotated.size, Size::new(3, 2));

        r = Rotate::Degrees270;
        source_bounds = Size::new(4, 8);
        let rotated = r.rotate_rectangle(rect, source_bounds);
        // (1, 1) in [4, 8] becomes (1, 2) in [8, 4].
        // The old top right is (3, 1), which becomes (1, 0).
        assert_eq!(rotated.top_left, Point::new(1, 0));
        assert_eq!(rotated.size, Size::new(2, 3));
    }
}
