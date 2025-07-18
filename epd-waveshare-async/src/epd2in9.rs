use core::time::Duration;
use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{Dimensions, Point, Size},
    primitives::Rectangle,
};
use embedded_hal::{
    digital::{InputPin, OutputPin},
    spi::{Phase, Polarity},
};
use embedded_hal_async::{delay::DelayNs, digital::Wait, spi::SpiDevice};

use crate::{
    buffer::{binary_buffer_length, BinaryBuffer},
    log::{debug, trace},
    Epd, EpdHw,
};

/// LUT for a full refresh. This should be used occasionally for best display results.
///
/// See [RECOMMENDED_MIN_FULL_REFRESH_INTERVAL] and [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
const LUT_FULL_UPDATE: [u8; 30] = [
    0x50, 0xAA, 0x55, 0xAA, 0x11, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x1F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
/// LUT for a partial refresh. This should be used for frequent updates, but it's recommended to
/// perform a full refresh occasionally.
///
/// See [RECOMMENDED_MIN_FULL_REFRESH_INTERVAL] and [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
const LUT_PARTIAL_UPDATE: [u8; 30] = [
    0x10, 0x18, 0x18, 0x08, 0x18, 0x18, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x13, 0x14, 0x44, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The refresh mode for the display.
pub enum RefreshMode {
    /// Use the full update LUT. This is slower, but should be done occasionally to avoid ghosting.
    ///
    /// It's recommended to avoid full refreshes less than [RECOMMENDED_MIN_FULL_REFRESH_INTERVAL] apart,
    /// but to do a full refresh at least every [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
    Full,
    /// Uses the partial update LUT for fast refresh. A full refresh should be done occasionally to
    /// avoid ghosting, see [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
    ///
    /// This is the standard "fast" update. It diffs the current framebuffer against the
    /// previous framebuffer, and just updates the pixels that differ.
    Partial,
    /// Uses the partial update LUT for a fast refresh, but only updates black (`BinaryColor::Off`)
    /// pixels from the current framebuffer. The previous framebuffer is ignored.
    PartialBlackBypass,
    /// Uses the partial update LUT for a fast refresh, but only updates white (`BinaryColor::On`)
    /// pixels from the current framebuffer. The previous framebuffer is ignored.
    PartialWhiteBypass,
}

impl RefreshMode {
    /// Returns the LUT to use for this refresh mode.
    pub fn lut(&self) -> &[u8; 30] {
        match self {
            RefreshMode::Full => &LUT_FULL_UPDATE,
            _ => &LUT_PARTIAL_UPDATE,
        }
    }
}

/// The height of the display (portrait orientation).
pub const DISPLAY_HEIGHT: u16 = 296;
/// The width of the display (portrait orientation).
pub const DISPLAY_WIDTH: u16 = 128;
/// It's recommended to avoid doing a full refresh more often than this (at least on a regular basis).
pub const RECOMMENDED_MIN_FULL_REFRESH_INTERVAL: Duration = Duration::from_secs(180);
/// It's recommended to do a full refresh at least this often.
pub const RECOMMENDED_MAX_FULL_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
pub const RECOMMENDED_SPI_HZ: u32 = 4_000_000; // 4 MHz
/// Use this phase in conjunction with [RECOMMENDED_SPI_POLARITY] so that the EPD can capture data
/// on the rising edge.
pub const RECOMMENDED_SPI_PHASE: Phase = Phase::CaptureOnFirstTransition;
/// Use this polarity in conjunction with [RECOMMENDED_SPI_PHASE] so that the EPD can capture data
/// on the rising edge.
pub const RECOMMENDED_SPI_POLARITY: Polarity = Polarity::IdleLow;

/// Low-level commands for the Epd2In9. You probably want to use the other methods exposed on the
/// [Epd2In9] for most operations, but can send commands directly with [Epd2In9::send] for low-level
/// control or experimentation.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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
    /// Activates the display update sequence. This must be set beforehand using [Command::DisplayUpdateControl2].
    /// This operation must not be interrupted.
    MasterActivation = 0x20,
    /// Used for a RAM "bypass" mode when using [RefreshMode::Partial]. This is poorly explained in the docs,
    /// but essentially we have three options:
    ///
    /// 1. `0x00` (default): just update the pixels that have changed **between the two internal
    ///    frame buffers**. This normally does what you expect. You can hack it a bit to do
    ///    interesting things by writing to both the old and new frame buffers.
    /// 2. `0x80`: just update the white (`BinaryColor::On`) pixels in the current frame buffer. It
    ///    doesn't matter what is in the old frame buffer.
    /// 3. `0x90`: just update the black (`BinaryColor::Off`) pixels in the current frame buffer.
    ///    It doesn't matter what is in the old frame buffer.
    ///
    /// Options 2 and 3 are what the datasheet calls "bypass" mode.
    DisplayUpdateControl1 = 0x21,
    /// Configures the display update sequence for use with [Command::MasterActivation].
    DisplayUpdateControl2 = 0x22,
    /// Writes data to the current frame buffer, auto-incrementing the address counter.
    WriteRam = 0x24,
    /// Writes data to the old frame buffer, auto-incrementing the address counter.
    WriteOldRam = 0x26,
    /// Writes to the VCOM register.
    WriteVcom = 0x2C,
    /// Writes the LUT register (30 bytes, exclude the VSH/VSL and dummy bits).
    WriteLut = 0x32,
    /// ? Part of magic config.
    SetDummyLinePeriod = 0x3A,
    /// ? Part of magic config.
    SetGateLineWidth = 0x3B,
    /// Register to configure the behaviour of the border.
    BorderWaveformControl = 0x3C,
    /// Sets the start and end positions of the X axis for the auto-incrementing address counter.
    /// Note that the x position can only be configured as a multiple of 8.
    SetRamXStartEnd = 0x44,
    /// Sets the start and end positions of the Y axis for the auto-incrementing address counter.
    SetRamYStartEnd = 0x45,
    /// Sets the current x coordinate of the address counter.
    /// Note that the x position can only be configured as a multiple of 8.
    SetRamX = 0x4E,
    /// Sets the current y coordinate of the address counter.
    SetRamY = 0x4F,
    /// Does nothing, but can be used to terminate other commands such as [Command::WriteRam]
    Noop = 0xFF,
}

impl Command {
    /// Returns the register address for this command.
    fn register(&self) -> u8 {
        *self as u8
    }
}

/// The buffer type used by [Epd2In9].
pub type Epd2In9Buffer =
    BinaryBuffer<{ binary_buffer_length(Size::new(DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32)) }>;

/// This should be sent with [Command::DriverOutputControl] during initialisation.
///
/// From the sample code, the bytes mean the following:
///
/// * low byte of display long edge
/// * high byte of display long edge
/// * GD = 0, SM = 0, TB = 0 (unclear what this means)
const DRIVER_OUTPUT_INIT_DATA: [u8; 3] = [0x27, 0x01, 0x00];
/// This should be sent with [Command::BoosterSoftStartControl] during initialisation.
/// Note that there are two versions of this command, one in the datasheet, and one in the sample code.
const BOOSTER_SOFT_START_INIT_DATA: [u8; 3] = [0xD7, 0xD6, 0x9D];
// Sample code: ^
// Datasheet:
// const BOOSTER_SOFT_START_INIT_DATA: [u8; 3] = [0xCF, 0xCE, 0x8D];

/// Controls v1 of the 2.9" Waveshare e-paper display.
///
/// * [datasheet](https://files.waveshare.com/upload/e/e6/2.9inch_e-Paper_Datasheet.pdf)
/// * [sample code](https://github.com/waveshareteam/e-Paper/blob/master/RaspberryPi_JetsonNano/python/lib/waveshare_epd/epd2in9.py)
///
/// The display has a portrait orientation. This uses [BinaryColor], where `Off` is black and `On` is white.
pub struct Epd2In9<HW>
where
    HW: EpdHw,
{
    hw: HW,
    refresh_mode: Option<RefreshMode>,
}

impl<HW> Epd2In9<HW>
where
    HW: EpdHw,
{
    pub fn new(hw: HW) -> Self {
        Epd2In9 {
            hw,
            refresh_mode: None,
        }
    }

    /// Sets the border to the specified colour. You need to call [Epd::update_display] using
    /// [RefreshMode::Full] afterwards to apply this change.
    ///
    /// Note: on my board, the white setting fades to grey fairly quickly. I have not found a way
    /// to avoid this.
    pub async fn set_border(
        &mut self,
        spi: &mut HW::Spi,
        color: BinaryColor,
    ) -> Result<(), HW::Error> {
        let border_setting: u8 = match color {
            BinaryColor::Off => 0x00,
            BinaryColor::On => 0x01,
        };
        self.send(spi, Command::BorderWaveformControl, &[border_setting])
            .await
    }

    /// Writes buffer data into the old internal framebuffer. This can be useful either:
    ///
    /// * to prep the next frame before the current one has been displayed (since the old buffer
    ///   becomes the current buffer after the next call to [Self::update_display()]).
    /// * to modify the "diff" that is displayed if in [RefreshMode::Partial]. Also see [Command::DisplayUpdateControl1].
    pub async fn write_old_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buffer: &Epd2In9Buffer,
    ) -> Result<(), <HW as EpdHw>::Error> {
        let buffer_bounds = buffer.bounding_box();
        self.set_window(spi, buffer_bounds).await?;
        self.set_cursor(spi, buffer_bounds.top_left).await?;
        self.send(spi, Command::WriteOldRam, buffer.data()).await
    }

    /// Send the following command and data to the display. Waits until the display is no longer busy before sending.
    pub async fn send(
        &mut self,
        spi: &mut HW::Spi,
        command: Command,
        data: &[u8],
    ) -> Result<(), HW::Error> {
        trace!("Sending EPD command: {:?}", command);
        self.wait_if_busy().await?;

        self.hw.dc().set_low()?;
        spi.write(&[command.register()]).await?;

        if !data.is_empty() {
            self.hw.dc().set_high()?;
            spi.write(data).await?;
        }

        Ok(())
    }

    /// Waits for the current operation to complete if the display is busy.
    ///
    /// Note that this will wait forever if the display is asleep.
    async fn wait_if_busy(&mut self) -> Result<(), HW::Error> {
        let busy = self.hw.busy();
        // Note: the datasheet states that busy pin is active low, i.e. we should wait for it when
        // it's low, but this is incorrect. The sample code treats it as active high, which works.
        if busy.is_high().unwrap() {
            trace!("Waiting for busy EPD");
            busy.wait_for_low().await?;
        }
        Ok(())
    }
}

impl<HW> Epd<HW> for Epd2In9<HW>
where
    HW: EpdHw,
{
    type RefreshMode = RefreshMode;
    type Buffer = Epd2In9Buffer;

    fn new_buffer(&self) -> Self::Buffer {
        BinaryBuffer::new(Size::new(DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32))
    }

    fn width(&self) -> u32 {
        DISPLAY_WIDTH as u32
    }

    fn height(&self) -> u32 {
        DISPLAY_HEIGHT as u32
    }

    async fn init(&mut self, spi: &mut HW::Spi, mode: RefreshMode) -> Result<(), HW::Error> {
        debug!("Initialising display");
        self.reset().await?;

        // Reset all configurations to default.
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
        self.send(spi, Command::DataEntryModeSetting, &[0b11])
            .await?;

        // Apply more magical config settings from the sample code.
        // Potentially: configure VCOM for 7 degrees celsius?
        self.send(spi, Command::WriteVcom, &[0xA8]).await?;
        // Configure 4 dummy lines per gate.
        self.send(spi, Command::SetDummyLinePeriod, &[0x1A]).await?;
        // 2us per line.
        self.send(spi, Command::SetGateLineWidth, &[0x08]).await?;

        self.set_refresh_mode(spi, mode).await
    }

    async fn set_refresh_mode(
        &mut self,
        spi: &mut <HW as EpdHw>::Spi,
        mode: Self::RefreshMode,
    ) -> Result<(), <HW as EpdHw>::Error> {
        // Update the LUT only if needed.
        match self.refresh_mode {
            Some(old_mode) if old_mode == mode => return Ok(()),
            Some(old_mode) if old_mode.lut() != mode.lut() => {
                self.send(spi, Command::WriteLut, mode.lut()).await?;
            }
            None => {
                self.send(spi, Command::WriteLut, mode.lut()).await?;
            }
            _ => {}
        }

        debug!("Changing refresh mode to {:?}", mode);
        self.refresh_mode = Some(mode);

        // Update bypass if needed.
        match mode {
            RefreshMode::Partial => {
                self.send(spi, Command::DisplayUpdateControl1, &[0x00])
                    .await
            }
            RefreshMode::PartialBlackBypass => {
                self.send(spi, Command::DisplayUpdateControl1, &[0x90])
                    .await
            }
            RefreshMode::PartialWhiteBypass => {
                self.send(spi, Command::DisplayUpdateControl1, &[0x80])
                    .await
            }
            _ => Ok(()),
        }
    }

    async fn reset(&mut self) -> Result<(), HW::Error> {
        debug!("Resetting EPD");
        // Assume reset is already high.
        self.hw.reset().set_low()?;
        self.hw.delay().delay_ms(10).await;
        self.hw.reset().set_high()?;
        self.hw.delay().delay_ms(10).await;
        Ok(())
    }

    async fn sleep(&mut self, spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        debug!("Sleeping EPD");
        self.send(spi, Command::DeepSleepMode, &[0x01]).await
    }

    async fn wake(&mut self, _spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        debug!("Waking EPD");
        self.reset().await

        // Confirmed with a physical screen that init is not required after waking.
    }

    async fn display_buffer(
        &mut self,
        spi: &mut HW::Spi,
        buffer: &Self::Buffer,
    ) -> Result<(), <HW as EpdHw>::Error> {
        self.write_framebuffer(spi, buffer).await?;

        self.update_display(spi).await
    }

    async fn write_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buffer: &Self::Buffer,
    ) -> Result<(), <HW as EpdHw>::Error> {
        let buffer_bounds = buffer.bounding_box();
        self.set_window(spi, buffer_bounds).await?;
        self.set_cursor(spi, buffer_bounds.top_left).await?;
        self.write_image(spi, buffer.data()).await
    }

    /// Sets the window to which the next image data will be written.
    ///
    /// The x-axis only supports multiples of 8; values outside this result in a debug-mode panic,
    /// or potentially misaligned content when debug assertions are disabled.
    async fn set_window(
        &mut self,
        spi: &mut HW::Spi,
        shape: Rectangle,
    ) -> Result<(), <HW as EpdHw>::Error> {
        // Use a debug assert as this is a soft failure in production; it will just lead to
        // slightly misaligned display content.
        let x_start = shape.top_left.x;
        let x_end = x_start + shape.size.width as i32 - 1;
        #[cfg(feature = "defmt")]
        defmt::debug_assert!(
            x_start % 8 == 0 && x_end % 8 == 7,
            "window's top_left.x and width must be 8-bit aligned"
        );
        #[cfg(not(feature = "defmt"))]
        debug_assert!(
            x_start % 8 == 0 && x_end % 8 == 7,
            "window's top_left.x and width must be 8-bit aligned"
        );
        let x_start_byte = ((x_start >> 3) & 0xFF) as u8;
        let x_end_byte = ((x_end >> 3) & 0xFF) as u8;
        self.send(spi, Command::SetRamXStartEnd, &[x_start_byte, x_end_byte])
            .await?;

        let (y_start_low, y_start_high) = split_low_and_high(shape.top_left.y as u16);
        let (y_end_low, y_end_high) =
            split_low_and_high((shape.top_left.y + shape.size.height as i32 - 1) as u16);
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
    /// The x-axis only supports multiples of 8; values outside this will result in a panic in
    /// debug mode, or potentially misaligned content if debug assertions are disabled.
    async fn set_cursor(
        &mut self,
        spi: &mut HW::Spi,
        position: Point,
    ) -> Result<(), <HW as EpdHw>::Error> {
        // Use a debug assert as this is a soft failure in production; it will just lead to
        // slightly misaligned display content.
        #[cfg(feature = "defmt")]
        defmt::debug_assert_eq!(position.x % 8, 0, "position.x must be 8-bit aligned");
        #[cfg(not(feature = "defmt"))]
        debug_assert_eq!(position.x % 8, 0, "position.x must be 8-bit aligned");

        self.send(spi, Command::SetRamX, &[(position.x >> 3) as u8])
            .await?;
        let (y_low, y_high) = split_low_and_high(position.y as u16);
        self.send(spi, Command::SetRamY, &[y_low, y_high]).await?;
        Ok(())
    }

    async fn update_display(&mut self, spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        // Enable the clock and CP (?), and then display the data from the RAM. Note that there are
        // two RAM buffers, so this will swap the active buffer. Calling this function twice in a row
        // without writing further to RAM therefore results in displaying the previous image.

        // Experimentation:
        // * Sending just 0x04 doesn't work, it hangs in busy state. The clocks are needed.
        // * Sending 0xC8 (INITIAL_DISPLAY) results in a black screen.
        // * Sending 0xCD (INITIAL_DISPLAY + PATTERN_DISPLAY) results in seemingly broken, semi-random behaviour.
        // The INIITIAL_DISPLAY settings potentially relate to the "bypass" settings in
        // [Command::DisplayUpdateControl1], but the precise mode is unclear.
        debug!("Updating display");

        self.send(spi, Command::DisplayUpdateControl2, &[0xC4])
            .await?;
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
}

#[inline(always)]
/// Splits a 16-bit value into the two 8-bit values representing the low and high bytes.
fn split_low_and_high(value: u16) -> (u8, u8) {
    let low = (value & 0xFF) as u8;
    let high = ((value >> 8) & 0xFF) as u8;
    (low, high)
}
