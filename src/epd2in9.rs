use core::time::Duration;
use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{Dimensions, Point, Size},
    primitives::Rectangle,
};
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal_async::{delay::DelayNs, digital::Wait, spi::SpiBus};

use crate::{
    buffer::{binary_buffer_length, BinaryBuffer},
    Epd, EpdHw, Error,
};

/// LUT for a full refresh. This should be used occasionally for best display results.
///
/// See [RECOMMENDED_MIN_FULL_REFRESH_INTERVAL] and [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
pub const LUT_FULL_UPDATE: [u8; 30] = [
    0x50, 0xAA, 0x55, 0xAA, 0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x1F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
/// LUT for a partial refresh. This should be used for frequent updates, but it's recommended to
/// perform a full refresh occasionally.
///
/// See [RECOMMENDED_MIN_FULL_REFRESH_INTERVAL] and [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
pub const LUT_PARTIAL_UPDATE: [u8; 30] = [
    0x10, 0x18, 0x18, 0x08, 0x18, 0x18, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x13, 0x14, 0x44, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The height of the display (portrait orientation).
pub const DISPLAY_HEIGHT: u16 = 296;
/// The width of the display (portrait orientation).
pub const DISPLAY_WIDTH: u16 = 128;
/// It's recommended to avoid doing a full refresh more often than this (at least on a regular basis).
pub const RECOMMENDED_MIN_FULL_REFRESH_INTERVAL: Duration = Duration::from_secs(180);
/// It's recommended to do a full refresh at least this often.
pub const RECOMMENDED_MAX_FULL_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Used to initialise the display.
    DriverOutputControl = 0x01,
    /// Used to configure the on chip voltage booster and regulator.
    BoosterSoftStartControl = 0x0C,
    /// Used to enter deep sleep mode. Requires a hardware reset and reinitialisation to wake up.
    DeepSleepMode = 0x10,
    /// Changes the auto-increment behaviour of the address counter.
    DataEntryModeSetting = 0x11,
    /// Resets all commands and parameters to default values (except deep sleep mode).
    SwReset = 0x12,
    /// Writes to the temperature register.
    TemperatureSensorControl = 0x1A,
    /// Activates the display update sequence. This must be set beforehand using [DisplayUpdateControl2].
    /// This operation must not be interrupted.
    MasterActivation = 0x20,
    /// Can be used to bypass the RAM content and directly read 1 or 0 for all pixels.
    DisplayUpdateControl1 = 0x21,
    /// Configures the display update sequence for use with [MasterActivation].
    DisplayUpdateControl2 = 0x22,
    /// Writes data to RAM, autoincrementing the address counter.
    WriteRam = 0x24,
    /// Writes to the VCOM register.
    WriteVcom = 0x2C,
    /// Writes the LUT register (30 bytes, exclude the VSH/VSL and dummy bits).
    WriteLut = 0x32,
    /// ? Part of magic config.
    SetDummyLinePeriod = 0x3A,
    /// ? Part of magic config.
    SetGateLineWidth = 0x3B,
    /// Register to configure the behaviour of the border.
    /// This can be set directly to a fixed colour, or managed via the VCOM register.
    BorderWaveformControl = 0x3C,
    /// Sets the start and end positions of the X axis.
    SetRamXStartEnd = 0x44,
    /// Sets the start and end positions of the Y axis.
    SetRamYStartEnd = 0x45,
    /// Sets the current x coordinate of the address counter.
    SetRamX = 0x4E,
    /// Sets the current y coordinate of the address counter.
    SetRamY = 0x4F,
    /// Does nothing, but can be used to terminate other commands such as [WriteRam]
    Noop = 0xFF,
}

/// This should be sent with [Command::DriverOutputControl] during initialisation.
///
/// From the sample code, the bytes mean the following:
///
/// * low byte of display long edge
/// * high byte of display long edge
/// * GD = 0, SM = 0, TB = 0 (unclear what this means)
const DRIVER_OUTPUT_INIT_DATA: [u8; 3] = [0x27, 0x01, 0x00];
/// This should be sent with [Command::BoosterSoftStartControl] during initialisation.
/// Note: this comes from the datasheet, but doesn't match the booster data sent in the sample code.
/// That uses [0xD7, 0xD6, 0x9D]
const BOOSTER_SOFT_START_INIT_DATA: [u8; 3] = [0xCF, 0xCE, 0x8D];

impl Command {
    fn register(&self) -> u8 {
        *self as u8
    }
}

/// Controls v1 of the 2.9" Waveshare e-paper display ([datasheet](https://files.waveshare.com/upload/e/e6/2.9inch_e-Paper_Datasheet.pdf)).
///
/// Initialise the display with either [LUT_FULL_UPDATE] or [LUT_PARTIAL_UPDATE].
///
/// Defaults to a portrait orientation. Uses [BinaryColor], where `Off` is black and `On` is white.
pub struct Epd2in9<HW>
where
    HW: EpdHw,
{
    hw: HW,
}

impl<HW> Epd2in9<HW>
where
    HW: EpdHw,
{
    pub fn new(hw: HW) -> Self {
        Epd2in9 { hw }
    }

    /// Sets the border to the specified colour.
    pub async fn set_border(&mut self, color: BinaryColor) -> Result<(), HW::Error> {
        let border_setting: u8 = match color {
            BinaryColor::Off => 0x40, // Ground for black
            BinaryColor::On => 0x50,  // Set high for white
        };
        self.send(Command::BorderWaveformControl, &[border_setting])
            .await
    }
}

impl<HW> Epd<HW> for Epd2in9<HW>
where
    HW: EpdHw,
{
    type Command = Command;
    type Buffer = BinaryBuffer<
        { binary_buffer_length(Size::new(DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32)) },
    >;

    async fn init(&mut self, spi: &mut HW::Spi, lut: &[u8]) -> Result<(), HW::Error> {
        if lut.len() != 30 {
            Err(Error::InvalidArgument)?
        }

        // Ensure reset is high.
        self.hw.reset().set_high()?;

        // Reset everything to defaults.
        self.send(spi, Command::SwReset, &[]).await?;

        self.send(spi, Command::DriverOutputControl, &DRIVER_OUTPUT_INIT_DATA)
            .await?;
        self.send(
            spi,
            Command::BoosterSoftStartControl,
            &BOOSTER_SOFT_START_INIT_DATA,
        )
        .await?;
        // Auto-increment X and Y, moving in the X direction first.
        self.send(spi, Command::DataEntryModeSetting, &[0b11]).await?;

        // Apply more magical config settings from the sample code.
        self.send(spi, Command::WriteVcom, &[0xA8]).await?;
        // Configure 4 dummy lines per gate.
        self.send(spi, Command::SetDummyLinePeriod, &[0x1A]).await?;
        // 2us per line.
        self.send(spi, Command::SetGateLineWidth, &[0x08]).await?;

        self.send(spi, Command::WriteLut, lut).await?;

        Ok(())
    }

    async fn clear(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error> {
        // Bypass the RAM to read 1 (white) for all values. This should be faster than re-writing
        // all the display data.
        self.send(spi, Command::DisplayUpdateControl1, &[0x90]).await?;
        self.update_display(spi).await?;
        // Disable bypass for future commands.
        self.send(spi, Command::DisplayUpdateControl1, &[0x01]).await?;

        Ok(())
    }

    async fn reset(&mut self) -> Result<(), HW::Error> {
        // Assume reset is already high.
        self.hw.reset().set_low()?;
        self.hw.delay().delay_ms(10).await;
        self.hw.reset().set_high()?;
        self.hw.delay().delay_ms(10).await;
        Ok(())
    }

    async fn sleep(&mut self, spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        self.send(spi, Command::DeepSleepMode, &[0x01]).await
    }

    async fn wake(&mut self, _spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        self.reset().await

        // TODO: is init needed?
    }

    async fn display_buffer(
        &mut self,
        spi: &mut HW::Spi,
        buffer: &Self::Buffer,
    ) -> Result<(), <HW as EpdHw>::Error> {
        let buffer_bounds = buffer.bounding_box();
        self.set_window(spi, buffer_bounds).await?;
        self.set_cursor(spi, buffer_bounds.top_left).await?;
        self.write_image(spi, buffer.data()).await?;

        Ok(())
    }

    /// Sets the window to which the next image data will be written.
    ///
    /// The x-axis only supports multiples of 8; values outside this result in an [Error::InvalidArgument] error.
    async fn set_window(
        &mut self,
        spi: &mut HW::Spi,
        shape: Rectangle,
    ) -> Result<(), <HW as EpdHw>::Error> {
        let x_start = shape.top_left.x;
        let x_end = x_start + shape.size.width as i32;
        if x_start % 8 != 0 || x_end % 8 != 0 {
            Err(Error::InvalidArgument)?
        }
        let x_start_byte = (x_start >> 3) as u8;
        let x_end_byte = (x_end >> 8) as u8;
        self.send(spi, Command::SetRamXStartEnd, &[x_start_byte, x_end_byte])
            .await?;

        let y_start = shape.top_left.y;
        let y_end = y_start + shape.size.height as i32;
        let y_start_low = (y_start & 0xFF) as u8;
        let y_start_high = ((y_start >> 8) & 0xFF) as u8;
        let y_end_low = (y_end & 0xFF) as u8;
        let y_end_high = ((y_end >> 8) & 0xFF) as u8;
        self.send(
            spi,
            Command::SetRamYStartEnd,
            &[y_start_low, y_start_high, y_end_low, y_end_high],
        )
        .await?;

        Ok(())
    }

    /// Sets the cursor position to write the next data to.
    ///
    /// The x-axis only supports multiples of 8; values outside this will result in [Error::InvalidArgument].
    async fn set_cursor(
        &mut self,
        spi: &mut HW::Spi,
        position: Point,
    ) -> Result<(), <HW as EpdHw>::Error> {
        if position.x % 8 != 0 {
            Err(Error::InvalidArgument)?
        }
        self.send(spi, Command::SetRamX, &[(position.x >> 3) as u8])
            .await?;
        let y_low = (position.y & 0xFF) as u8;
        let y_high = ((position.y >> 8) & 0xFF) as u8;
        self.send(spi, Command::SetRamY, &[y_low, y_high]).await?;
        Ok(())
    }

    async fn update_display(&mut self, spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        // Enable the clock and CP (?), and then display the latest data.
        // self.send(Command::DisplayUpdateControl2, &[0xC4]).await?;
        // To try: just display the pattern
        self.send(spi, Command::DisplayUpdateControl2, &[0x04]).await?;
        self.send(spi, Command::MasterActivation, &[]).await?;
        self.send(spi, Command::Noop, &[]).await?;
        Ok(())
    }

    async fn write_image(
        &mut self,
        spi: &mut HW::Spi,
        image: &[u8],
    ) -> Result<(), <HW as EpdHw>::Error> {
        self.send(spi, Command::WriteRam, image).await
    }

    async fn send(
        &mut self,
        spi: &mut HW::Spi,
        command: Command,
        data: &[u8],
    ) -> Result<(), HW::Error> {
        self.wait_if_busy().await?;

        self.hw.cs().set_low()?;
        self.hw.dc().set_low()?;
        self.hw.spi().write(&[command.register()]).await?;

        if !data.is_empty() {
            self.hw.dc().set_high()?;
            self.hw.spi().write(data).await?;
        }

        self.hw.cs().set_high()?;
        Ok(())
    }

    async fn wait_if_busy(&mut self) -> Result<(), HW::Error> {
        let busy = self.hw.busy();
        if busy.is_low().unwrap() {
            busy.wait_for_high().await?;
        }
        Ok(())
    }
}

// Notes:

// Pull CS low to communicate.
// Pull D/C low for command, then high for data.
// Reset is active low.
// Busy is active low, only do things when it's high.
//
