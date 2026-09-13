use core::time::Duration;
use embedded_graphics::{
    prelude::{Point, Size},
    primitives::Rectangle,
};
use embedded_hal::{
    digital::{OutputPin, PinState},
    spi::{Phase, Polarity},
};
use embedded_hal_async::delay::DelayNs;

use crate::{
    buffer::{
        binary_buffer_length, split_low_and_high, BinaryBuffer, BufferView, Gray2SplitBuffer,
    },
    hw::{BusyHw, CommandDataSend, DcHw, DelayHw, ErrorHw, ResetHw, SpiHw},
    log::{debug, debug_assert},
    DisplayPartial, DisplaySimple, Displayable, Reset, Sleep, Wake,
};

const LUT_GRAY2: [u8; 233] = [
    0x01, 0x0A, 0x1B, 0x0F, 0x03, 0x01, 0x01, 0x05, 0x0A, 0x01, 0x0A, 0x01, 0x01, 0x01, 0x05, 0x08,
    0x03, 0x02, 0x04, 0x01, 0x01, 0x01, 0x04, 0x04, 0x02, 0x00, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00,
    0x00, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x0A, 0x1B, 0x0F, 0x03, 0x01,
    0x01, 0x05, 0x4A, 0x01, 0x8A, 0x01, 0x01, 0x01, 0x05, 0x48, 0x03, 0x82, 0x84, 0x01, 0x01, 0x01,
    0x84, 0x84, 0x82, 0x00, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x01, 0x01, 0x0A, 0x1B, 0x8F, 0x03, 0x01, 0x01, 0x05, 0x4A, 0x01, 0x8A, 0x01,
    0x01, 0x01, 0x05, 0x48, 0x83, 0x82, 0x04, 0x01, 0x01, 0x01, 0x04, 0x04, 0x02, 0x00, 0x01, 0x01,
    0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x8A,
    0x1B, 0x8F, 0x03, 0x01, 0x01, 0x05, 0x4A, 0x01, 0x8A, 0x01, 0x01, 0x01, 0x05, 0x48, 0x83, 0x02,
    0x04, 0x01, 0x01, 0x01, 0x04, 0x04, 0x02, 0x00, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01,
    0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x8A, 0x9B, 0x8F, 0x03, 0x01, 0x01, 0x05,
    0x4A, 0x01, 0x8A, 0x01, 0x01, 0x01, 0x05, 0x48, 0x03, 0x42, 0x04, 0x01, 0x01, 0x01, 0x04, 0x04,
    0x42, 0x00, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x07, 0x17, 0x41, 0xA8, 0x32, 0x30,
];

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The refresh mode for the display.
pub enum RefreshMode {
    /// A full-screen refresh using the fast waveform, selected by writing to the temperature sensor
    /// register. This is quicker than [RefreshMode::Full], but may result in more ghosting. If
    /// ghosting persists, try [RefreshMode::Full].
    Fast,
    /// A full-screen refresh using the display's OTP waveform. This gives the cleanest image and
    /// should be done occasionally to avoid ghosting. It's the slowest refresh mode.
    ///
    /// It's recommended to avoid full refreshes less than [RECOMMENDED_MIN_FULL_REFRESH_INTERVAL] apart,
    /// but to do a full refresh at least every [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
    Full,
    /// Uses the display's partial update waveform for a fast refresh. A full refresh should be done
    /// occasionally to avoid ghosting, see [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
    ///
    /// This is the fastest update. It diffs the current framebuffer (written to the low RAM) against
    /// the base framebuffer (written to the high RAM), and only drives the pixels where they differ.
    /// The base framebuffer must therefore hold the image that is currently on screen, or pixels that
    /// are unchanged between the two framebuffers will keep their previous on-screen state. Write the
    /// base with [DisplayPartial::write_base_framebuffer] using the last displayed image after each
    /// partial update (or use a full/fast refresh to display a whole new image).
    Partial,
    /// A refresh mode that supports 2-bit grayscale. Note that Waveshare calls this "Gray4", but
    /// we use `Gray2` to align with the embedded-graphics color [embedded_graphics::pixelcolor::Gray2].
    ///
    /// There is no partial update version for Gray2. All updates require writing to both on-device framebuffers.
    Gray2,
}

impl RefreshMode {
    /// Returns the border waveform setting to use for this refresh mode.
    pub fn border_waveform(&self) -> &'static [u8] {
        match self {
            RefreshMode::Full | RefreshMode::Fast => &[0x05],
            RefreshMode::Partial => &[0x80],
            RefreshMode::Gray2 => &[0x03],
        }
    }

    /// Returns the value to set for [Command::DisplayUpdateControl2] when using this refresh mode.
    pub fn display_update_control_2(&self) -> &[u8] {
        match self {
            RefreshMode::Fast => &[0xC7],
            RefreshMode::Full => &[0xF7],
            RefreshMode::Partial => &[0xFF],
            RefreshMode::Gray2 => &[0xCF],
        }
    }

    /// If this refresh mode is black and white only.
    pub fn is_black_and_white(&self) -> bool {
        *self != RefreshMode::Gray2
    }
}

/// Selects the border waveform, overriding the refresh mode's default.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderWaveform {
    /// Uses the reference value for the current refresh mode (`0x05` for full/fast, `0x80` for
    /// partial, `0x03` for Gray2).
    Default,
    /// Fixes the border at a plain black level (`0x50`).
    Black,
    /// Fixes the border at a plain white level (`0x60`).
    White,
}

impl BorderWaveform {
    /// Returns the border waveform byte for the given refresh mode, honouring this override.
    fn value_for(&self, mode: RefreshMode) -> &'static [u8] {
        match (self, mode) {
            (BorderWaveform::Default, _) => mode.border_waveform(),
            (BorderWaveform::Black, _) => &[0x50],
            (BorderWaveform::White, _) => &[0x60],
        }
    }
}

/// The height of the display.
pub const DISPLAY_HEIGHT: u32 = 300;
/// The width of the display.
pub const DISPLAY_WIDTH: u32 = 400;
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
/// The default pin state that indicates the display is busy.
pub const DEFAULT_BUSY_WHEN: PinState = PinState::High;

/// Low-level commands for the Epd4In2 v2 display. You probably want to use the other methods
/// exposed on the [Epd4In2V2] for most operations, but can send commands directly with [Epd4In2V2::send] for low-level
/// control or experimentation.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Sets the driver output control (gate scan direction and gate output count).
    DriverOutputControl = 0x01,
    /// Sets the gate driving voltage (standard value: 0x00, or 0x17).
    SetGateDrivingVoltage = 0x03,
    /// Sets the source driving voltage (standard value: [0x41, 0xA8, 0x32]).
    SetSourceDrivingVoltage = 0x04,
    /// Booster Control used for Gray2 initialization.
    BoosterSoftStart = 0x0C,
    /// Used to enter deep sleep mode. Requires a hardware reset and reinitialisation to wake up.
    DeepSleepMode = 0x10,
    /// Changes the auto-increment behaviour of the address counter.
    DataEntryModeSetting = 0x11,
    /// Resets all commands and parameters to default values (except deep sleep mode).
    SwReset = 0x12,
    /// Selects the temperature-based waveform speed used for fast refreshes (`0x5A` = 1s, `0x6E` = 1.5s).
    TemperatureSensorControl = 0x1A,
    /// Activates the display update sequence. This must be set beforehand using [Command::DisplayUpdateControl2].
    /// This operation must not be interrupted.
    MasterActivation = 0x20,
    /// Sets the display update sequence to use with [Command::MasterActivation]. The value is
    /// `[0x40, 0x00]` for full and fast refreshes, and `[0x00, 0x00]` for partial and Gray2 refreshes.
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

    /// Reads OTP registers (sections: VCOM OTP selection, VCOM register, Display Mode, Waveform Version).
    ReadOtpRegisters = 0x2D,
    /// Reads the 10 byte User ID stored in OTP.
    ReadUserId = 0x2E,
    /// Programs the Waveform Setting OTP (requires writing the bytes into RAM first). Requires
    /// CLKEN to have been enabled via [Command::DisplayUpdateControl2].
    ProgramWsOtp = 0x30,
    /// Loads the Waveform Setting OTP. Requires CLKEN to have been enabled via
    /// [Command::DisplayUpdateControl2].
    LoadWsOtp = 0x31,

    /// Writes the LUT register. For the 4.2" V2 display this is 227 bytes, containing the Gray2 waveform.
    WriteLut = 0x32,

    /// Programs the OTP selection according to the OTP selection control (registers 0x37 and 0x38).
    /// Requires CLKEN to have been enabled via [Command::DisplayUpdateControl2].
    ProgramOtpSelection = 0x36,

    /// Writes the register for the user ID that can be stored in the OTP.
    WriteRegisterForUserId = 0x38,
    /// Sets the OTP program mode:
    ///
    /// * 0x00: normal mode
    /// * 0x03: internally generated OTP programming voltage
    SetOtpProgramMode = 0x39,
    /// Sets the border waveform for the display update (`0x05` for full/fast, `0x80` for partial,
    /// `0x03` for Gray2).
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

/// The length of the underlying buffer used by [Epd4In2V2].
pub const BINARY_BUFFER_LENGTH: usize =
    binary_buffer_length(Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT));
/// The buffer type used by [Epd4In2V2].
pub type Epd4In2BinaryBuffer = BinaryBuffer<BINARY_BUFFER_LENGTH>;
/// Constructs a new binary buffer for use with the [Epd4In2V2] display.
pub const fn new_binary_buffer() -> Epd4In2BinaryBuffer {
    Epd4In2BinaryBuffer::new(Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT))
}
pub type Epd4In2Gray2Buffer = Gray2SplitBuffer<BINARY_BUFFER_LENGTH>;
pub const fn new_gray2_buffer() -> Epd4In2Gray2Buffer {
    Epd4In2Gray2Buffer::new(Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT))
}

/// Controls v2 of the 4.2" Waveshare e-paper display.
///
/// * [datasheet](https://files.waveshare.com/upload/9/97/4.2-inch-e-Paper-V2-user-manual.pdf)
/// * [sample code](https://github.com/waveshareteam/e-Paper/blob/master/RaspberryPi_JetsonNano/python/lib/waveshare_epd/epd4in2_V2.py)
///
/// This display supports either
/// [embedded_graphics::pixelcolor::BinaryColor] or [embedded_graphics::pixelcolor::Gray2],
/// depending on the display mode.
///
/// When using `BinaryColor`, `Off` is black and `On` is white.
///
/// HW should implement [ResetHw], [BusyHw], [DcHw], [SpiHw], [DelayHw], and [ErrorHw].
pub struct Epd4In2V2<HW, STATE> {
    hw: HW,
    state: STATE,
    border_waveform: BorderWaveform,
}

trait StateInternal {}
#[allow(private_bounds)]
pub trait State: StateInternal {}
pub trait StateAwake: State {}

macro_rules! impl_base_state {
    ($state:ident) => {
        impl StateInternal for $state {}
        impl State for $state {}
    };
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateUninitialized();
impl_base_state!(StateUninitialized);
impl StateAwake for StateUninitialized {}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateReady {
    mode: RefreshMode,
}
impl_base_state!(StateReady);
impl StateAwake for StateReady {}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateAsleep<W: StateAwake> {
    wake_state: W,
}
impl<W: StateAwake> StateInternal for StateAsleep<W> {}
impl<W: StateAwake> State for StateAsleep<W> {}

impl<HW> Epd4In2V2<HW, StateUninitialized>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    pub fn new(hw: HW) -> Self {
        Epd4In2V2 {
            hw,
            state: StateUninitialized(),
            border_waveform: BorderWaveform::Default,
        }
    }
}

impl<HW, STATE> Epd4In2V2<HW, STATE>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
    STATE: StateAwake,
{
    /// Initialises the display.
    pub async fn init(
        mut self,
        spi: &mut HW::Spi,
        mode: RefreshMode,
    ) -> Result<Epd4In2V2<HW, StateReady>, HW::Error> {
        debug!("Initializing display to {}", mode);
        self = self.reset().await?;

        let mut epd = Epd4In2V2 {
            hw: self.hw,
            state: StateReady { mode },
            border_waveform: self.border_waveform,
        };

        epd.set_refresh_mode_impl(spi, mode).await?;
        Ok(epd)
    }
}

impl<HW, STATE> Epd4In2V2<HW, STATE>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
    STATE: StateAwake,
{
    /// Send the following command and data to the display. Waits until the display is no longer busy before sending.
    pub async fn send(
        &mut self,
        spi: &mut HW::Spi,
        command: Command,
        data: &[u8],
    ) -> Result<(), HW::Error> {
        self.hw.send(spi, command.register(), data).await
    }
}

impl<HW> Epd4In2V2<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    /// Sets the refresh mode.
    pub async fn set_refresh_mode(
        &mut self,
        spi: &mut HW::Spi,
        mode: RefreshMode,
    ) -> Result<(), HW::Error> {
        if self.state.mode == mode {
            Ok(())
        } else {
            debug!("Changing refresh mode to {:?}", mode);
            self.set_refresh_mode_impl(spi, mode).await?;
            Ok(())
        }
    }

    async fn set_refresh_mode_impl(
        &mut self,
        spi: &mut HW::Spi,
        mode: RefreshMode,
    ) -> Result<(), HW::Error> {
        let border = self.border_waveform.value_for(mode);
        match mode {
            RefreshMode::Full => {
                self.send(spi, Command::SwReset, &[]).await?;
                self.send(spi, Command::DisplayUpdateControl1, &[0x40, 0x00])
                    .await?;
                self.send(spi, Command::SetBorderWaveform, border).await?;
                self.set_window_regs(spi).await?;
            }
            RefreshMode::Fast => {
                self.send(spi, Command::SwReset, &[]).await?;
                self.send(spi, Command::DisplayUpdateControl1, &[0x40, 0x00])
                    .await?;
                self.send(spi, Command::SetBorderWaveform, border).await?;
                // Select the fast waveform via temperature, then load it.
                // Value 0x5A also works, however produces worse contrast
                // Lines 213-218 (https://github.com/waveshareteam/e-Paper/blob/master/RaspberryPi_JetsonNano/python/lib/waveshare_epd/epd4in2_V2.py)
                self.send(spi, Command::TemperatureSensorControl, &[0x6E]) // 0x6E = 1.5s variant
                    .await?;
                self.send(spi, Command::DisplayUpdateControl2, &[0x91])
                    .await?;
                self.send(spi, Command::MasterActivation, &[]).await?;
                self.set_window_regs(spi).await?;
            }
            RefreshMode::Partial => {
                self.send(spi, Command::SwReset, &[]).await?;
                self.send(spi, Command::SetBorderWaveform, border).await?;
                self.send(spi, Command::DisplayUpdateControl1, &[0x00, 0x00])
                    .await?;
                self.set_window_regs(spi).await?;
            }
            RefreshMode::Gray2 => {
                self.send(spi, Command::SwReset, &[]).await?;
                self.send(spi, Command::DisplayUpdateControl1, &[0x00, 0x00])
                    .await?;
                self.send(spi, Command::SetBorderWaveform, border).await?;
                self.send(spi, Command::BoosterSoftStart, &[0x8B, 0x9C, 0xA4, 0x0F])
                    .await?;
                self.send(spi, Command::WriteLut, &LUT_GRAY2[..227]).await?;
                self.send(spi, Command::SetLutMagic, &LUT_GRAY2[227..228])
                    .await?;
                self.send(spi, Command::SetGateDrivingVoltage, &LUT_GRAY2[228..229])
                    .await?;
                self.send(spi, Command::SetSourceDrivingVoltage, &LUT_GRAY2[229..232])
                    .await?;
                self.send(spi, Command::WriteVcom, &LUT_GRAY2[232..])
                    .await?;
                self.set_window_regs(spi).await?;
            }
        }

        self.state.mode = mode;
        Ok(())
    }

    /// Changes the border waveform override and applies it to the display. The new border is used
    /// for the next display update, and on any subsequent mode changes. Set to
    /// [BorderWaveform::Default] to revert to the refresh mode's reference value.
    pub async fn set_border_waveform(
        &mut self,
        spi: &mut HW::Spi,
        border_waveform: BorderWaveform,
    ) -> Result<(), HW::Error> {
        self.border_waveform = border_waveform;
        let border = border_waveform.value_for(self.state.mode);
        self.send(spi, Command::SetBorderWaveform, border).await
    }

    /// Sets the RAM window and cursor to cover the full 400x300 display
    /// (x: 0..399 -> `0x31`, y: 0..299 -> `0x2B`/`0x01`).
    async fn set_window_regs(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error> {
        self.send(spi, Command::DataEntryModeSetting, &[0b11])
            .await?;
        self.send(spi, Command::SetRamXStartEnd, &[0x00, 0x31])
            .await?;
        self.send(spi, Command::SetRamYStartEnd, &[0x00, 0x00, 0x2B, 0x01])
            .await?;
        self.send(spi, Command::SetRamX, &[0x00]).await?;
        self.send(spi, Command::SetRamY, &[0x00, 0x00]).await?;
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
    ) -> Result<(), HW::Error> {
        let x_start = shape.top_left.x;
        let x_end = shape.top_left.x + shape.size.width as i32 - 1;
        // Use a debug assert as this is a soft failure in production; it will just lead to
        // slightly misaligned display content.
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
    ) -> Result<(), HW::Error> {
        // Use a debug assert as this is a soft failure in production; it will just lead to
        // slightly misaligned display content.
        debug_assert_eq!(position.x % 8, 0, "position.x must be 8-bit aligned");

        self.send(spi, Command::SetRamX, &[(position.x >> 3) as u8])
            .await?;
        let (y_low, y_high) = split_low_and_high(position.y as u16);
        self.send(spi, Command::SetRamY, &[y_low, y_high]).await?;
        Ok(())
    }
}

async fn reset_impl<HW>(hw: &mut HW) -> Result<(), HW::Error>
where
    HW: ResetHw + DelayHw + ErrorHw,
    HW::Error: From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>,
{
    debug!("Resetting EPD");
    hw.reset().set_high()?;
    hw.delay().delay_ms(100).await;
    hw.reset().set_low()?;
    hw.delay().delay_ms(2).await;
    hw.reset().set_high()?;
    hw.delay().delay_ms(100).await;
    Ok(())
}

impl<HW, STATE: StateAwake> Reset<HW::Error> for Epd4In2V2<HW, STATE>
where
    HW: ResetHw + DelayHw + ErrorHw,
    HW::Error: From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>,
{
    type DisplayOut = Epd4In2V2<HW, STATE>;

    async fn reset(mut self) -> Result<Self::DisplayOut, HW::Error> {
        reset_impl(&mut self.hw).await?;
        Ok(self)
    }
}

impl<HW, W: StateAwake> Reset<HW::Error> for Epd4In2V2<HW, StateAsleep<W>>
where
    HW: ResetHw + DelayHw + ErrorHw,
    HW::Error: From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>,
{
    type DisplayOut = Epd4In2V2<HW, W>;

    async fn reset(mut self) -> Result<Self::DisplayOut, HW::Error> {
        reset_impl(&mut self.hw).await?;
        Ok(Epd4In2V2 {
            hw: self.hw,
            state: self.state.wake_state,
            border_waveform: self.border_waveform,
        })
    }
}

impl<HW, STATE: StateAwake> Sleep<HW::Spi, HW::Error> for Epd4In2V2<HW, STATE>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    type DisplayOut = Epd4In2V2<HW, StateAsleep<STATE>>;

    async fn sleep(mut self, spi: &mut HW::Spi) -> Result<Self::DisplayOut, HW::Error> {
        debug!("Sleeping EPD");
        self.send(spi, Command::DeepSleepMode, &[0x01]).await?;
        Ok(Epd4In2V2 {
            hw: self.hw,
            state: StateAsleep {
                wake_state: self.state,
            },
            border_waveform: self.border_waveform,
        })
    }
}

impl<HW> Wake<HW::Spi, HW::Error> for Epd4In2V2<HW, StateAsleep<StateUninitialized>>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    type DisplayOut = Epd4In2V2<HW, StateUninitialized>;
    async fn wake(self, _spi: &mut HW::Spi) -> Result<Self::DisplayOut, HW::Error> {
        debug!("Waking EPD");
        // No refresh mode has been configured yet, so there's nothing to re-initialise; the
        // caller is expected to call `init` next.
        self.reset().await
    }
}

impl<HW> Wake<HW::Spi, HW::Error> for Epd4In2V2<HW, StateAsleep<StateReady>>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    type DisplayOut = Epd4In2V2<HW, StateReady>;
    async fn wake(self, spi: &mut HW::Spi) -> Result<Self::DisplayOut, HW::Error> {
        debug!("Waking EPD");
        let mode = self.state.wake_state.mode;
        let mut epd = self.reset().await?;
        // Deep sleep clears the display's configuration registers, so they must be resent.
        epd.set_refresh_mode_impl(spi, mode).await?;
        Ok(epd)
    }
}

impl<HW> Displayable<HW::Spi, HW::Error> for Epd4In2V2<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    async fn update_display(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error> {
        debug!("Updating display");

        let mode = self.state.mode;
        let update_control = mode.display_update_control_2();
        self.send(spi, Command::DisplayUpdateControl2, update_control)
            .await?;

        self.send(spi, Command::MasterActivation, &[]).await?;
        Ok(())
    }
}

impl<HW> DisplaySimple<1, 1, HW::Spi, HW::Error> for Epd4In2V2<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
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
        self.send(spi, Command::WriteLowRam, buf.data()[0]).await?;
        if self.state.mode != RefreshMode::Partial {
            self.send(spi, Command::WriteHighRam, buf.data()[0]).await?;
        }
        Ok(())
    }
}

impl<HW> DisplaySimple<1, 2, HW::Spi, HW::Error> for Epd4In2V2<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    async fn display_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buf: &dyn BufferView<1, 2>,
    ) -> Result<(), HW::Error> {
        self.write_framebuffer(spi, buf).await?;

        self.update_display(spi).await
    }

    async fn write_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buf: &dyn BufferView<1, 2>,
    ) -> Result<(), HW::Error> {
        let buffer_bounds = buf.window();
        self.set_window(spi, buffer_bounds).await?;
        self.set_cursor(spi, buffer_bounds.top_left).await?;
        self.send(spi, Command::WriteLowRam, buf.data()[0]).await?;
        self.send(spi, Command::WriteHighRam, buf.data()[1]).await
    }
}

impl<HW> DisplayPartial<1, 1, HW::Spi, HW::Error> for Epd4In2V2<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    async fn write_base_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buf: &dyn BufferView<1, 1>,
    ) -> Result<(), HW::Error> {
        let buffer_bounds = buf.window();
        self.set_window(spi, buffer_bounds).await?;
        self.set_cursor(spi, buffer_bounds.top_left).await?;
        self.send(spi, Command::WriteHighRam, buf.data()[0]).await
    }
}
