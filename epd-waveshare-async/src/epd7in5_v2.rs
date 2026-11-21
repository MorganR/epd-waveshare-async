use core::time::Duration;
use embedded_graphics::{geometry::Point, prelude::Size, primitives::Rectangle};
use embedded_hal::{
    digital::{OutputPin, PinState},
    spi::{Phase, Polarity},
};
use embedded_hal_async::delay::DelayNs;

use crate::{
    buffer::{binary_buffer_length, BinaryBuffer, BufferView, Gray2SplitBuffer},
    hw::{BusyHw, BusyWait, CommandDataSend, DcHw, DelayHw, ErrorHw, ResetHw, SpiHw},
    log::debug,
    Clear, DisplayPartial, DisplaySimple, Displayable, Reset, Sleep,
};

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The refresh mode for the display.
pub enum RefreshMode {
    /// Refreshes the entire display. This is slower than [RefreshMode::Partial], but should be done
    /// occasionally to avoid ghosting. If ghosting persists, try [RefreshMode::Full].
    ///
    /// It's recommended to avoid full refreshes less than [RECOMMENDED_MIN_FULL_REFRESH_INTERVAL] apart,
    /// but to do a full refresh at least every [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
    Fast,
    /// A slower full update that gives a cleaner final image.
    ///
    /// It's recommended to avoid full refreshes less than [RECOMMENDED_MIN_FULL_REFRESH_INTERVAL] apart,
    /// but to do a full refresh at least every [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
    Full,
    /// Changes only specific areas of the screen with no flickering.
    /// A fast/full refresh should be done occasionally to avoid ghosting,
    /// see [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
    ///
    /// It diffs the current framebuffer against the
    /// previous framebuffer, and just updates the pixels that differ.
    Partial,
    /// A refresh mode that supports 2-bit grayscale. Note that Waveshare calls this "Gray4", but
    /// we use `Gray2` to align with the embedded-graphics color [embedded_graphics::pixelcolor::Gray2].
    ///
    /// There is no partial update version for Gray2. All updates require writing to both on-device framebuffers.
    Gray2,
}

impl RefreshMode {
    /// If this refresh mode is black and white only.
    pub fn is_black_and_white(&self) -> bool {
        *self != RefreshMode::Gray2
    }
}

/// The width of the display (landscape orientation).
pub const DISPLAY_WIDTH: u32 = 800;
/// The height of the display (landscape orientation).
pub const DISPLAY_HEIGHT: u32 = 480;
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
pub const DEFAULT_BUSY_WHEN: PinState = PinState::Low;

/// Low-level commands for the Epd7in5 v2 display. You probably want to use the other methods
/// exposed on the [Epd7in5] for most operations, but can send commands directly with [Epd7in5::send] for low-level
/// control or experimentation.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    PanelSet = 0x00,
    PowerSet = 0x01,
    PowerOff = 0x02,
    PowerOffSequenceSet = 0x03,
    PowerOn = 0x04,
    PowerOnMeasure = 0x05,
    BoosterSoftStart = 0x06,
    DeepSleep = 0x07,
    DisplayStartTrans1 = 0x10, // Front buffer
    DataStop = 0x11,
    DisplayRefresh = 0x12,
    DisplayStartTrans2 = 0x13, // Back buffer
    DualSPI = 0x15,
    AutoSequence = 0x17,
    KWLUTOption = 0x2B,
    PLLControl = 0x30,
    TempSensorCalibration = 0x40,
    TempSensorSelect = 0x41,
    TempSensorWrite = 0x42,
    TempSensorRead = 0x43,
    PanelBreakCheck = 0x44,
    VCOMDataInterval = 0x50,
    LowerPowerDetection = 0x51,
    EndVoltageSet = 0x52,
    TCONSet = 0x60,
    ResolutionSet = 0x61,
    GateSourceStartSet = 0x65,
    Revision = 0x70,
    GetStatus = 0x71,
    AutoMeasurementVCOM = 0x80,
    ReadVCOM = 0x81,
    VCOMDCSetting = 0x82,
    PartialWindow = 0x90,
    PartialIn = 0x91,
    PartialOut = 0x92,
    ProgramMode = 0xA0,
    ActiveProgramming = 0xA1,
    ReadOTP = 0xA2,
    CascadeSet = 0xE0,
    PowerSaving = 0xE3,
    LVDVoltageSelect = 0xE4,
    ForceTemperature = 0xE5,
    TempBoundaryPhaseC2 = 0xE7,
}

impl Command {
    /// Returns the register address for this command.
    fn register(&self) -> u8 {
        *self as u8
    }
}

/// The length of the underlying buffer used by [Epd7in5].
pub const BINARY_BUFFER_LENGTH: usize =
    binary_buffer_length(Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT));
/// The buffer type used by [Epd7in5].
pub type Epd7In5BinaryBuffer = BinaryBuffer<BINARY_BUFFER_LENGTH>;
/// Constructs a new binary buffer for use with the [Epd7in5] display.
pub const fn new_binary_buffer() -> Epd7In5BinaryBuffer {
    Epd7In5BinaryBuffer::new(Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT))
}
pub type Epd7In5Gray2Buffer = Gray2SplitBuffer<BINARY_BUFFER_LENGTH>;
pub const fn new_gray2_buffer() -> Epd7In5Gray2Buffer {
    Epd7In5Gray2Buffer::new(Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT))
}

/// Controls v2 of the 7.5" Waveshare e-paper display.
///
/// * [datasheet](https://files.waveshare.com/upload/6/60/7.5inch_e-Paper_V2_Specification.pdf)
/// * [sample code](https://github.com/waveshareteam/e-Paper/blob/master/Arduino_R4/src/e-Paper/EPD_7in5_V2.cpp)
///
/// The display has a landscape orientation. This display supports either
/// [embedded_graphics::pixelcolor::BinaryColor] or [embedded_graphics::pixelcolor::Gray2],
/// depending on the display mode.
///
/// When using `BinaryColor`, `Off` is black and `On` is white.
///
/// HW should implement [ResetHw], [BusyHw], [DcHw], [SpiHw], [DelayHw], and [ErrorHw].
pub struct Epd7In5V2<HW, STATE> {
    hw: HW,
    state: STATE,
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

/// The display's state is unknown, e.g. on start up, or after something went wrong. The only
/// valid operation is [Reset::reset].
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateUnknown();
impl_base_state!(StateUnknown);

/// The display has been hardware reset, and must be initialised with [Epd7In5V2::init].
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

impl<HW> Epd7In5V2<HW, StateUnknown>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    /// Creates a new display in an unknown state. Call [Reset::reset] followed by
    /// [Epd7In5V2::init] to prepare it for use.
    pub fn new(hw: HW) -> Self {
        Epd7In5V2 {
            hw,
            state: StateUnknown(),
        }
    }
}

impl<HW> Epd7In5V2<HW, StateUninitialized>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    /// Initialises the display after a [Reset::reset].
    pub async fn init(
        self,
        spi: &mut HW::Spi,
        mode: RefreshMode,
    ) -> Result<Epd7In5V2<HW, StateReady>, HW::Error> {
        debug!("Initializing display to {}", mode);

        let mut epd = Epd7In5V2 {
            hw: self.hw,
            state: StateReady { mode },
        };

        epd.set_refresh_mode_impl(spi, mode).await?;
        Ok(epd)
    }
}

impl<HW, STATE> Epd7In5V2<HW, STATE>
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

impl<HW> Epd7In5V2<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw + DelayHw + ResetHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>,
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
            reset_impl(&mut self.hw).await?;
            self.set_refresh_mode_impl(spi, mode).await?;
            Ok(())
        }
    }

    async fn set_refresh_mode_impl(
        &mut self,
        spi: &mut HW::Spi,
        mode: RefreshMode,
    ) -> Result<(), HW::Error> {
        match mode {
            RefreshMode::Fast => {
                // PANEL SETTING
                self.send(spi, Command::PanelSet, &[0x1F]).await?;

                // VCOM DATA INTERVAL
                self.send(spi, Command::VCOMDataInterval, &[0x10, 0x07])
                    .await?;

                // If the screen appears gray, use the annotated initialization command

                // POWER ON
                self.send(spi, Command::PowerOn, &[]).await?;
                self.hw.delay().delay_ms(100).await;
                self.hw.wait_if_busy().await?;

                // BOOSTER + Cascade + Force Temp
                self.send(spi, Command::BoosterSoftStart, &[0x27, 0x27, 0x18, 0x17])
                    .await?;
                self.send(spi, Command::CascadeSet, &[0x02]).await?;
                self.send(spi, Command::ForceTemperature, &[0x5A]).await?;
            }
            RefreshMode::Full => {
                // POWER SETTING
                self.send(spi, Command::PowerSet, &[0x07, 0x07, 0x3F, 0x3F])
                    .await?;

                // BOOSTER SOFT START
                self.send(spi, Command::BoosterSoftStart, &[0x17, 0x17, 0x28, 0x17])
                    .await?;

                // POWER ON
                self.send(spi, Command::PowerOn, &[]).await?;
                self.hw.delay().delay_ms(100).await;
                self.hw.wait_if_busy().await?;

                // PANEL SETTING
                self.send(spi, Command::PanelSet, &[0x1F]).await?;

                // RESOLUTION SETTING (TRES)
                self.send(spi, Command::ResolutionSet, &[0x03, 0x20, 0x01, 0xE0])
                    .await?;

                // DUAL SPI
                self.send(spi, Command::DualSPI, &[0x00]).await?;

                // VCOM DATA INTERVAL
                self.send(spi, Command::VCOMDataInterval, &[0x10, 0x07])
                    .await?;

                // If the screen appears gray, use the annotated initialization command

                // TCON SETTING
                self.send(spi, Command::TCONSet, &[0x22]).await?;
            }
            RefreshMode::Partial => {
                // PANEL SETTING
                self.send(spi, Command::PanelSet, &[0x1F]).await?;

                // POWER ON
                self.send(spi, Command::PowerOn, &[]).await?;
                self.hw.delay().delay_ms(100).await;
                self.hw.wait_if_busy().await?;

                // Cascade + Force Temp
                self.send(spi, Command::CascadeSet, &[0x02]).await?;
                self.send(spi, Command::ForceTemperature, &[0x6E]).await?;
            }
            RefreshMode::Gray2 => {
                // PANEL SETTING
                self.send(spi, Command::PanelSet, &[0x1F]).await?;

                // VCOM DATA INTERVAL
                self.send(spi, Command::VCOMDataInterval, &[0x10, 0x07])
                    .await?;

                // POWER ON
                self.send(spi, Command::PowerOn, &[]).await?;
                self.hw.delay().delay_ms(100).await;
                self.hw.wait_if_busy().await?;

                // BOOSTER + Cascade + Force Temp
                self.send(spi, Command::BoosterSoftStart, &[0x27, 0x27, 0x18, 0x17])
                    .await?;
                self.send(spi, Command::CascadeSet, &[0x02]).await?;
                self.send(spi, Command::ForceTemperature, &[0x5F]).await?;
            }
        }

        self.state.mode = mode;

        Ok(())
    }

    /// Sets the window to which the next image data will be written.
    pub async fn set_window(
        &mut self,
        spi: &mut HW::Spi,
        shape: Rectangle,
    ) -> Result<(), HW::Error> {
        let Point {
            x: x_start,
            y: y_start,
        } = shape.top_left;
        let Point { x: x_end, y: y_end } = shape.bottom_right().unwrap();

        self.send(spi, Command::PartialIn, &[]).await?;

        let window: [u8; _] = [
            (x_start / 256) as u8,
            (x_start % 256) as u8,
            (x_end / 256) as u8,
            (x_end % 256) as u8 - 1,
            (y_start / 256) as u8,
            (y_start % 256) as u8,
            (y_end / 256) as u8,
            (y_end % 256) as u8 - 1,
            0x01,
        ];

        self.send(spi, Command::PartialWindow, &window).await?;

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
    hw.delay().delay_ms(20).await;
    hw.reset().set_low()?;
    hw.delay().delay_ms(2).await;
    hw.reset().set_high()?;
    hw.delay().delay_ms(200).await;
    Ok(())
}

impl<HW, STATE: State> Reset<HW::Error> for Epd7In5V2<HW, STATE>
where
    HW: ResetHw + DelayHw + ErrorHw,
    HW::Error: From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>,
{
    type DisplayOut = Epd7In5V2<HW, StateUninitialized>;

    async fn reset(mut self) -> Result<Self::DisplayOut, HW::Error> {
        reset_impl(&mut self.hw).await?;
        Ok(Epd7In5V2 {
            hw: self.hw,
            state: StateUninitialized(),
        })
    }
}

impl<HW, STATE: StateAwake> Sleep<HW::Spi, HW::Error> for Epd7In5V2<HW, STATE>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    type DisplayOut = Epd7In5V2<HW, StateAsleep<StateUninitialized>>;

    async fn sleep(mut self, spi: &mut HW::Spi) -> Result<Self::DisplayOut, HW::Error> {
        debug!("Sleeping EPD");
        self.send(spi, Command::VCOMDataInterval, &[0xF7]).await?;
        self.send(spi, Command::PowerOff, &[]).await?;
        self.send(spi, Command::DeepSleep, &[0xA5]).await?;
        Ok(Epd7In5V2 {
            hw: self.hw,
            state: StateAsleep {
                wake_state: StateUninitialized(),
            },
        })
    }
}

impl<HW> Displayable<HW::Spi, HW::Error> for Epd7In5V2<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw + DelayHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    async fn update_display(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error> {
        debug!("Updating display");

        self.send(spi, Command::DisplayRefresh, &[]).await?;
        self.hw.delay().delay_ms(100).await;
        self.hw.wait_if_busy().await?;
        Ok(())
    }
}

impl<HW> Clear<HW::Spi, HW::Error> for Epd7In5V2<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw + DelayHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    async fn clear(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error> {
        let buf1_value;
        let buf2_value;
        match self.state.mode {
            RefreshMode::Fast | RefreshMode::Full | RefreshMode::Partial => {
                buf1_value = 0xFF;
                buf2_value = 0x00;
            }
            RefreshMode::Gray2 => {
                buf1_value = 0x00;
                buf2_value = 0x00;
            }
        };

        self.hw
            .send_iter(
                spi,
                Command::DisplayStartTrans1 as u8,
                Some(core::iter::repeat_n(buf1_value, BINARY_BUFFER_LENGTH)),
            )
            .await?;
        self.hw
            .send_iter(
                spi,
                Command::DisplayStartTrans2 as u8,
                Some(core::iter::repeat_n(buf2_value, BINARY_BUFFER_LENGTH)),
            )
            .await?;

        self.update_display(spi).await?;
        Ok(())
    }
}

impl<HW> DisplaySimple<1, 1, HW::Spi, HW::Error> for Epd7In5V2<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw + DelayHw,
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
        let data = buf.data()[0];
        self.send(spi, Command::DisplayStartTrans1, data).await?;
        self.hw
            .send_iter(
                spi,
                Command::DisplayStartTrans2 as u8,
                Some(data.iter().map(|px| !px)),
            )
            .await?;
        Ok(())
    }
}

impl<HW> DisplaySimple<1, 2, HW::Spi, HW::Error> for Epd7In5V2<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw + DelayHw,
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
        let data = buf.data();
        self.send(spi, Command::DisplayStartTrans1, data[0]).await?;
        self.send(spi, Command::DisplayStartTrans2, data[1]).await?;
        Ok(())
    }
}

impl<HW> DisplayPartial<1, 1, HW::Spi, HW::Error> for Epd7In5V2<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw + DelayHw + ResetHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>,
{
    async fn write_base_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buf: &dyn BufferView<1, 1>,
    ) -> Result<(), HW::Error> {
        let buffer_bounds = buf.window();

        let data = buf.data()[0];

        self.send(spi, Command::VCOMDataInterval, &[0xA9, 0x07])
            .await?;

        self.set_window(spi, buffer_bounds).await?;

        self.send(spi, Command::DisplayStartTrans2, data).await?;

        Ok(())
    }
}
