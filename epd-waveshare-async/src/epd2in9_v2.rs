use core::time::Duration;
use embedded_graphics::{
    prelude::{Point, Size},
    primitives::Rectangle,
};
use embedded_hal::{
    digital::OutputPin,
    spi::{Phase, Polarity},
};
use embedded_hal_async::delay::DelayNs;

use crate::{
    buffer::{binary_buffer_length, split_low_and_high, BinaryBuffer, BufferView}, hw::CommandDataSend as _, log::{debug, debug_assert, warn_log}, DisplayPartial, DisplaySimple, Displayable, EpdHw, Error, Reset, Sleep
};

/// LUT for a full refresh. This should be used occasionally for best display results.
///
/// See [RECOMMENDED_MIN_FULL_REFRESH_INTERVAL] and [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
const LUT_FULL_UPDATE: [u8; 153] = [
    0x90, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x60, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x90, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x60, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x19, 0x19, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x24, 0x42, 0x22, 0x22, 0x23, 0x32, 0x00, 0x00, 0x00,
];
const LUT_MAGIC_FULL_UPDATE: [u8; 1] = [0x22];
const GATE_VOLTAGE_FULL_UPDATE: [u8; 1] = [0x17];
const SOURCE_VOLTAGE_FULL_UPDATE: [u8; 3] = [0x41, 0xAE, 0x32];
const VCOM_FULL_UPDATE: [u8; 1] = [0x38];
/// LUT for a partial refresh. This should be used for frequent updates, but it's recommended to
/// perform a full refresh occasionally.
///
/// See [RECOMMENDED_MIN_FULL_REFRESH_INTERVAL] and [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
const LUT_PARTIAL_UPDATE: [u8; 153] = [
    0x0, 0x40, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x80, 0x80, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x40, 0x40, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x80, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0x0, 0x0, 0x0A, 0x0, 0x0, 0x0, 0x0, 0x0, 0x1, 0x1, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x1, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0,
    0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x22, 0x22, 0x22, 0x22, 0x22,
    0x22, 0x0, 0x0, 0x0,
];
const LUT_MAGIC_PARTIAL_UPDATE: [u8; 1] = [0x22];
const GATE_VOLTAGE_PARTIAL_UPDATE: [u8; 1] = [0x17];
const SOURCE_VOLTAGE_PARTIAL_UPDATE: [u8; 3] = [0x41, 0xB0, 0x32];
const VCOM_PARTIAL_UPDATE: [u8; 1] = [0x36];

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
    ///
    /// TODO: This doesn't work yet.
    Partial,
}

impl RefreshMode {
    /// Returns the border waveform setting to use for this refresh mode.
    pub fn border_waveform(&self) -> &[u8] {
        match self {
            RefreshMode::Full => &[0x05],
            RefreshMode::Partial => &[0x80],
            // Grey: 0x04
        }
    }

    /// Returns the LUT to use for this refresh mode.
    pub fn lut(&self) -> &[u8] {
        match self {
            RefreshMode::Full => &LUT_FULL_UPDATE,
            _ => &LUT_PARTIAL_UPDATE,
        }
    }

    pub fn lut_magic(&self) -> &[u8] {
        match self {
            RefreshMode::Full => &LUT_MAGIC_FULL_UPDATE,
            RefreshMode::Partial => &LUT_MAGIC_PARTIAL_UPDATE,
        }
    }

    pub fn gate_voltage(&self) -> &[u8] {
        match self {
            RefreshMode::Full => &GATE_VOLTAGE_FULL_UPDATE,
            RefreshMode::Partial => &GATE_VOLTAGE_PARTIAL_UPDATE,
        }
    }

    pub fn source_voltage(&self) -> &[u8] {
        match self {
            RefreshMode::Full => &SOURCE_VOLTAGE_FULL_UPDATE,
            RefreshMode::Partial => &SOURCE_VOLTAGE_PARTIAL_UPDATE,
        }
    }

    pub fn vcom(&self) -> &[u8] {
        match self {
            RefreshMode::Full => &VCOM_FULL_UPDATE,
            RefreshMode::Partial => &VCOM_PARTIAL_UPDATE,
        }
    }

    /// Returns the value to set for [Command::DisplayUpdateControl2] when using this refresh mode.
    pub fn display_update_control_2(&self) -> &[u8] {
        match self {
            // We use 0xCF (similar to 0x0F in sample code) because we need to enable clock and
            // analog. These are already enabled elsewhere in the sample code, but we do a slightly
            // different set up.
            RefreshMode::Partial => &[0xCF],
            _ => &[0xC7],
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

/// Low-level commands for the Epd2In9 v2 display. You probably want to use the other methods
/// exposed on the [Epd2In9V2] for most operations, but can send commands directly with [Epd2In9V2::send] for low-level
/// control or experimentation.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Used to initialise the display.
    DriverOutputControl = 0x01,
    /// Sets the gate driving voltage (standard value: 0x00, or 0x17).
    SetGateDrivingVoltage = 0x03,
    /// Sets the source driving voltage (standard value: [0x41, 0xA8, 0x32]).
    SetSourceDrivingVoltage = 0x04,
    /// Used to enter deep sleep mode. Requires a hardware reset and reinitialisation to wake up.
    DeepSleepMode = 0x10,
    /// Changes the auto-increment behaviour of the address counter.
    DataEntryModeSetting = 0x11,
    /// Resets all commands and parameters to default values (except deep sleep mode).
    SwReset = 0x12,
    /// Activates the display update sequence. This must be set beforehand using [Command::DisplayUpdateControl2].
    /// This operation must not be interrupted.
    MasterActivation = 0x20,
    /// Used for a RAM "bypass" mode when using [RefreshMode::Partial]. This is poorly explained in the docs,
    /// but essentially we have these options:
    ///
    /// In black and white mode:
    ///
    /// 1. `0x00` (default): just update the pixels that have changed **between the two internal
    ///    frame buffers**. This normally does what you expect. You can hack it a bit to do
    ///    interesting things by writing to both the old and new frame buffers.
    /// 2. `0x04`: just update the white (`BinaryColor::On`) pixels in the current frame buffer. It
    ///    doesn't matter what is in the old frame buffer.
    /// 3. `0x08`: just update the black (`BinaryColor::Off`) pixels in the current frame buffer.
    ///    It doesn't matter what is in the old frame buffer.
    ///
    /// In 4-color greyscale mode: same as above for the behaviour of the black and white bit, but
    /// OR-ed with:
    ///
    /// 1. `0x00` (default)
    /// 2. `0x40` (just update 1 bits)
    /// 3. `0x80` (just update 0 bits)
    ///
    /// TODO: verify the behaviour of greyscale mode.
    DisplayUpdateControl1 = 0x21,
    /// Configures the display update sequence for use with [Command::MasterActivation].
    DisplayUpdateControl2 = 0x22,
    /// Writes low bits to the current frame buffer.
    WriteLowRam = 0x24,
    /// Writes high bits to the current frame buffer.
    WriteHighRam = 0x26,
    /// Triggers a read of the VCOM voltage. Requires that CLKEN and ANALOGEN have been enabled via
    /// [Command::DisplayUpdateControl2].
    ReadVcom = 0x28,
    /// Sets the duration to hold before reading the VCOM value.
    SetVcomReadDuration = 0x29,
    /// Programs the VCOM register into the OTP. Requires that CLKEN has been enabled via
    /// [Command::DisplayUpdateControl2].
    ProgramVcomOtp = 0x2A,
    /// Writes to the VCOM register.
    WriteVcom = 0x2C,

    /// ?? Reads OTP registers (sections: VCOM OTP selection, VCOM register, Display Mode, Waveform Version).
    ReadOtpRegisters = 0x2D,
    /// ?? Reads 10 byte User ID stored in OTP.
    ReadUserId = 0x2E,
    /// ?? Programs the OTP of Waveform Setting (requires writing the bytes into RAM first). Requires
    /// CLKEN to have been enabled via [Command::DisplayUpdateControl2].
    ProgramWsOtp = 0x30,
    /// ?? Loads the OTP of Waveform Setting. Requires CLKEN to have been enabled via
    /// [Command::DisplayUpdateControl2].
    LoadWsOtp = 0x31,

    /// Writes the LUT register (153 bytes, containing VS[nX-LUTm], TP[nX], RP[n], SR[nXY], FR[n], and XON[nXY]).
    WriteLut = 0x32,

    /// ?? Programs OTP selection according to the OTP selection control (registers 0x37 and 0x38).
    /// Requires CLKEN to have been enabled via [Command::DisplayUpdateControl2].
    ProgramOtpSelection = 0x36,

    /// Undocumented command for writing OTP data.    
    /// Writes the register for the user ID that can be stored in the OTP.
    WriteRegisterForUserId = 0x38,
    /// ?? Sets the OTP program mode:
    ///
    /// * 0x00: normal mode
    /// * 0x03: internally generated OTP programming voltage
    SetOtpProgramMode = 0x39,
    /// Undocumented command used when initialising each refresh mode.
    SetBorderWaveform = 0x3C,
    /// Undocumented command needed for setting the LUT.
    SetLutMagic = 0x3F,

    /// Sets the start and end positions of the X axis for the auto-incrementing address counter.
    /// Start and end are inclusive.
    ///
    /// Note that the x position can only be written on a whole byte basis (8 bits at once). The
    /// start and end positions are therefore sent right shifted 3 bits to indicate the byte number
    /// being written. For example, to write the first 32 x positions, you would send 0 (0 >> 3 =
    /// 0), and 3 (31 >> 3 = 3). If you tried to write just the first 25 x positions, you would end
    /// up sending the same values and actually writing all 32.
    SetRamXStartEnd = 0x44,
    /// Sets the start and end positions of the Y axis for the auto-incrementing address counter.
    /// Start and end are inclusive.
    SetRamYStartEnd = 0x45,
    /// Sets the current x coordinate of the address counter.
    /// Note that the x position can only be configured as a multiple of 8.
    SetRamX = 0x4E,
    /// Sets the current y coordinate of the address counter.
    SetRamY = 0x4F,
}

impl Command {
    /// Returns the register address for this command.
    fn register(&self) -> u8 {
        *self as u8
    }
}

/// The length of the underlying buffer used by [Epd2In9V2].
pub const BINARY_BUFFER_LENGTH: usize = binary_buffer_length(Size::new(DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32));
/// The buffer type used by [Epd2In9V2].
pub type Epd2In9BinaryBuffer = BinaryBuffer<BINARY_BUFFER_LENGTH>;
/// Constructs a new binary buffer for use with the [Epd2In9V2] display.
pub fn new_binary_buffer() -> Epd2In9BinaryBuffer {
    Epd2In9BinaryBuffer::new(Size::new(DISPLAY_WIDTH as u32, DISPLAY_HEIGHT as u32))
}

/// This should be sent with [Command::DriverOutputControl] during initialisation.
///
/// From the sample code, the bytes mean the following:
///
/// * low byte of display long edge
/// * high byte of display long edge
/// * GD = 0, SM = 0, TB = 0 (unclear what this means)
const DRIVER_OUTPUT_INIT_DATA: [u8; 3] = [0x27, 0x01, 0x00];

/// Controls v2 of the 2.9" Waveshare e-paper display.
///
/// * [datasheet](https://files.waveshare.com/upload/7/79/2.9inch-e-paper-v2-specification.pdf)
/// * [sample code](https://github.com/waveshareteam/e-Paper/blob/master/RaspberryPi_JetsonNano/python/lib/waveshare_epd/epd2in9_V2.py)
///
/// The display has a portrait orientation. This uses [BinaryColor], where `Off` is black and `On` is white.
///
/// 4-color greyscale is not yet supported.
pub struct Epd2In9V2<HW>
where
    HW: EpdHw,
{
    hw: HW,
    refresh_mode: Option<RefreshMode>,
    state: State,
}

#[derive(PartialEq)]
enum State {
    Uninitialized,
    Awake,
    Asleep,
}

impl<HW> Epd2In9V2<HW>
where
    HW: EpdHw,
{
    pub fn new(hw: HW) -> Self {
        Epd2In9V2 {
            hw,
            refresh_mode: None,
            state: State::Uninitialized,
        }
    }

    pub async fn init(&mut self, spi: &mut HW::Spi, mode: RefreshMode) -> Result<(), HW::Error> {
        debug!("Initialising display");
        self.reset().await?;

        // Reset all configurations to default.
        self.send(spi, Command::SwReset, &[]).await?;

        self.send(spi, Command::DriverOutputControl, &DRIVER_OUTPUT_INIT_DATA)
            .await?;
        // Auto-increment X and Y, moving in the X direction first.
        self.send(spi, Command::DataEntryModeSetting, &[0b11])
            .await?;

        // Set to black and white mode.
        self.send(spi, Command::DisplayUpdateControl1, &[0x00, 0x80])
            .await?;

        self.set_refresh_mode(spi, mode).await
    }

    pub async fn set_refresh_mode(
        &mut self,
        spi: &mut <HW as EpdHw>::Spi,
        mode: RefreshMode,
    ) -> Result<(), <HW as EpdHw>::Error> {
        self.verify_awake_and_init()?;

        // Update the LUT only if needed.
        match self.refresh_mode {
            Some(old_mode) if old_mode == mode => return Ok(()),
            _ => {}
        }

        debug!("Changing refresh mode to {:?}", mode);
        self.refresh_mode = Some(mode);

        self.send(spi, Command::SetBorderWaveform, mode.border_waveform())
            .await?;

        self.send(spi, Command::WriteLut, mode.lut()).await?;
        self.send(spi, Command::SetLutMagic, mode.lut_magic())
            .await?;
        self.send(spi, Command::SetGateDrivingVoltage, mode.gate_voltage())
            .await?;
        self.send(spi, Command::SetSourceDrivingVoltage, mode.source_voltage())
            .await?;
        self.send(spi, Command::WriteVcom, mode.vcom()).await?;

        if mode == RefreshMode::Partial {
            // Mystery undocumented command from sample code.
            self.hw
                .send(
                    spi,
                    0x37,
                    &[0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00],
                )
                .await?;

            self.send(spi, Command::DisplayUpdateControl2, &[0xC3])
                .await?;
            self.send(spi, Command::MasterActivation, &[]).await?;
        }

        Ok(())
    }

    /// Sets the window to which the next image data will be written.
    ///
    /// The x-axis only supports multiples of 8; values outside this result in a debug-mode panic,
    /// or potentially misaligned content when debug assertions are disabled.
    pub async fn set_window(
        &mut self,
        spi: &mut HW::Spi,
        shape: Rectangle,
    ) -> Result<(), <HW as EpdHw>::Error> {
        self.verify_awake_and_init()?;

        // Use a debug assert as this is a soft failure in production; it will just lead to
        // slightly misaligned display content.
        let x_start = shape.top_left.x;
        let x_end = x_start + shape.size.width as i32 - 1;
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
    pub async fn set_cursor(
        &mut self,
        spi: &mut HW::Spi,
        position: Point,
    ) -> Result<(), <HW as EpdHw>::Error> {
        self.verify_awake_and_init()?;

        // Use a debug assert as this is a soft failure in production; it will just lead to
        // slightly misaligned display content.
        debug_assert_eq!(position.x % 8, 0, "position.x must be 8-bit aligned");

        self.send(spi, Command::SetRamX, &[(position.x >> 3) as u8])
            .await?;
        let (y_low, y_high) = split_low_and_high(position.y as u16);
        self.send(spi, Command::SetRamY, &[y_low, y_high]).await?;
        Ok(())
    }

    /// Send the following command and data to the display. Waits until the display is no longer busy before sending.
    pub async fn send(
        &mut self,
        spi: &mut HW::Spi,
        command: Command,
        data: &[u8],
    ) -> Result<(), HW::Error> {
        if self.state == State::Asleep {
            Err(Error::Sleeping)?;
        }

        self.hw.send(spi, command.register(), data).await
    }

    fn verify_awake_and_init(&self) -> Result<(), Error> {
        match self.state {
            State::Awake => Ok(()),
            State::Uninitialized => Err(Error::Uninitialized),
            State::Asleep => Err(Error::Sleeping)
        }
    }

    fn verify_partial_supported(&self) -> Result<(), Error> {
        match self.refresh_mode {
            Some(RefreshMode::Partial) => Ok(()),
            _ => Err(Error::WrongRefreshMode),
        }
    }
}

impl <HW: EpdHw> Reset<HW::Error> for Epd2In9V2<HW> {
    async fn reset(&mut self) -> Result<(), HW::Error> {
        debug!("Resetting EPD");
        // Assume reset is already high.
        self.hw.reset().set_low()?;
        self.hw.delay().delay_ms(10).await;
        self.hw.reset().set_high()?;
        self.hw.delay().delay_ms(10).await;
        self.state = State::Awake;
        Ok(())
    }
}

impl <HW: EpdHw> Sleep<HW::Spi, HW::Error> for Epd2In9V2<HW> {
    async fn sleep(&mut self, spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        if self.state == State::Asleep {
            return Ok(());
        }

        debug!("Sleeping EPD");
        self.send(spi, Command::DeepSleepMode, &[0x01]).await?;
        self.state = State::Asleep;
        Ok(())
    }

    async fn wake(&mut self, _spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        debug!("Waking EPD");
        self.reset().await
        // State is updated inside reset.
    }
}

impl <HW: EpdHw> Displayable<HW::Spi, HW::Error> for Epd2In9V2<HW> {
    async fn update_display(&mut self, spi: &mut HW::Spi) -> Result<(), <HW as EpdHw>::Error> {
        self.verify_awake_and_init()?;
        debug!("Updating display");

        if let Some(mode) = self.refresh_mode {
            self.send(
                spi,
                Command::DisplayUpdateControl2,
                mode.display_update_control_2(),
            )
            .await?;
        } else {
            warn_log!("Display used before being initialised");
        }

        self.send(spi, Command::MasterActivation, &[]).await?;
        Ok(())
    }
}

impl <HW: EpdHw> DisplaySimple<1, 1, HW::Spi, HW::Error> for Epd2In9V2<HW> {
    async fn display_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buf: &dyn BufferView<1, 1>,
    ) -> Result<(), HW::Error> {
        self.write_framebuffer(spi, buf).await?;

        self.update_display(spi).await
    }

    async fn write_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buf: &dyn BufferView<1, 1>,
    ) -> Result<(), HW::Error> {
        let buffer_bounds = buf.window();
        self.set_window(spi, buffer_bounds).await?;
        self.set_cursor(spi, buffer_bounds.top_left).await?;
        self.send(spi, Command::WriteLowRam, buf.data()[0]).await
    }
}

impl <HW: EpdHw> DisplayPartial<1, 1, HW::Spi, HW::Error> for Epd2In9V2<HW> {
    async fn write_base_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buf: &dyn BufferView<1, 1>,
    ) -> Result<(), HW::Error> {
        self.verify_partial_supported()?;
        // Awake and init is verified in window and cursor commands.
        let buffer_bounds = buf.window();
        self.set_window(spi, buffer_bounds).await?;
        self.set_cursor(spi, buffer_bounds.top_left).await?;
        self.send(spi, Command::WriteHighRam, buf.data()[0]).await
    }
}

