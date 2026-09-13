use core::time::Duration;
use embedded_graphics::prelude::Size;
use embedded_hal::{
    digital::{OutputPin, PinState},
    spi::{Phase, Polarity},
};
use embedded_hal_async::delay::DelayNs;

use crate::{
    buffer::{hex_buffer_length, BufferView, HexBuffer},
    hw::{BusyHw, BusyWait, CommandDataSend, DcHw, DelayHw, ErrorHw, ResetHw, SpiHw},
    log::debug,
    Clear, DisplaySimple, Displayable, Reset, Sleep,
};

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// The refresh mode for the display.
pub enum RefreshMode {
    /// A slow full update that gives a clean final image. This is the only supported refresh mode right now.
    ///
    /// It's recommended to avoid full refreshes less than [RECOMMENDED_MIN_FULL_REFRESH_INTERVAL] apart,
    /// but to do a full refresh at least every [RECOMMENDED_MAX_FULL_REFRESH_INTERVAL].
    Full,
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
    Ox05 = 0x05,
    BoosterSoftStart = 0x06,
    DeepSleep = 0x07,
    Ox08 = 0x08,
    DataStartTrans1 = 0x10,
    DataStop = 0x11,
    DisplayRefresh = 0x12,
    ImageProcessCommand = 0x13,
    PLLControl = 0x30,
    TempSensorCalibration = 0x40,
    TempSensorSelect = 0x41,
    TempSensorWrite = 0x42,
    TempeSensorRead = 0x43,
    VCOMDataInterval = 0x50,
    LowerPowerDetection = 0x51,
    TCONSet = 0x60,
    TCONResolution = 0x61,
    SPIFlashControl = 0x65,
    Revision = 0x70,
    GetStatus = 0x71,
    AutoMeasurementVCOM = 0x80,
    ReadVCOM = 0x81,
    VCOMDCSetting = 0x82,
    Ox84 = 0x84,
    CMDH = 0xAA,
    PowerSaving = 0xE3,
}
impl Command {
    /// Returns the register address for this command.
    fn register(&self) -> u8 {
        *self as u8
    }
}

/// The length of the underlying buffer used by [Epd7in5].
pub const HEX_BUFFER_LENGTH: usize = hex_buffer_length(Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT));
/// The buffer type used by [Epd7in5].
pub type Epd7In3EBuffer = HexBuffer<HEX_BUFFER_LENGTH>;
/// Constructs a new binary buffer for use with the [Epd7in5] display.
pub const fn new_hex_buffer() -> Epd7In3EBuffer {
    Epd7In3EBuffer::new(Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT))
}

/// Controls the 7.3" Waveshare Spectra 6 e-paper display.
///
/// * [user manual](https://files.waveshare.com/wiki/7.3inch-e-Paper-HAT-(E)/7.3inch-e-Paper-(E)-user-manual.pdf)
/// * [sample code](https://github.com/waveshareteam/e-Paper/blob/master/Arduino_R4/src/e-Paper/EPD_7in3e.cpp)
///
/// The display has a landscape orientation. This display only supports
/// [embedded_graphics::pixelcolor::Rgb888] at the moment.
///
/// HW should implement [ResetHw], [BusyHw], [DcHw], [SpiHw], [DelayHw], and [ErrorHw].
pub struct Epd7In3E<HW, STATE> {
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

impl<HW> Epd7In3E<HW, StateUninitialized>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    pub fn new(hw: HW) -> Self {
        Epd7In3E {
            hw,
            state: StateUninitialized(),
        }
    }
}

impl<HW, STATE> Epd7In3E<HW, STATE>
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
    ) -> Result<Epd7In3E<HW, StateReady>, HW::Error> {
        debug!("Initializing display to {}", mode);
        self = self.reset().await?;
        self.hw.wait_if_busy().await?;
        self.hw.delay().delay_ms(30).await;

        let mut epd = Epd7In3E {
            hw: self.hw,
            state: StateReady { mode },
        };

        epd.set_refresh_mode_impl(spi, mode).await?;
        Ok(epd)
    }
}

impl<HW, STATE> Epd7In3E<HW, STATE>
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

impl<HW> Epd7In3E<HW, StateReady>
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
            self.hw.wait_if_busy().await?;
            self.hw.delay().delay_ms(30).await;

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
            RefreshMode::Full => {
                self.send(spi, Command::CMDH, &[0x49, 0x55, 0x20, 0x08, 0x09, 0x18])
                    .await?;
                self.send(spi, Command::PowerSet, &[0x3F]).await?;
                self.send(spi, Command::PanelSet, &[0x5F, 0x69]).await?;
                self.send(spi, Command::PowerOffSequenceSet, &[0x00, 0x54, 0x00, 0x44])
                    .await?;
                self.send(spi, Command::Ox05, &[0x40, 0x1F, 0x1F, 0x2C])
                    .await?;
                self.send(spi, Command::BoosterSoftStart, &[0x6F, 0x1F, 0x17, 0x49])
                    .await?;
                self.send(spi, Command::Ox08, &[0x6F, 0x1F, 0x1F, 0x22])
                    .await?;
                self.send(spi, Command::PLLControl, &[0x03]).await?;
                self.send(spi, Command::VCOMDataInterval, &[0x3F]).await?;
                self.send(spi, Command::TCONSet, &[0x02, 0x00]).await?;
                self.send(spi, Command::TCONResolution, &[0x03, 0x20, 0x01, 0xE0])
                    .await?;
                self.send(spi, Command::Ox84, &[0x01]).await?;
                self.send(spi, Command::PowerSaving, &[0x2F]).await?;
            }
        }

        self.state.mode = mode;

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

impl<HW, STATE: StateAwake> Reset<HW::Error> for Epd7In3E<HW, STATE>
where
    HW: ResetHw + DelayHw + ErrorHw,
    HW::Error: From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>,
{
    type DisplayOut = Epd7In3E<HW, STATE>;

    async fn reset(mut self) -> Result<Self::DisplayOut, HW::Error> {
        reset_impl(&mut self.hw).await?;
        Ok(self)
    }
}

impl<HW, W: StateAwake> Reset<HW::Error> for Epd7In3E<HW, StateAsleep<W>>
where
    HW: ResetHw + DelayHw + ErrorHw,
    HW::Error: From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>,
{
    type DisplayOut = Epd7In3E<HW, W>;

    async fn reset(self) -> Result<Self::DisplayOut, HW::Error> {
        // will do reset inside init()
        Ok(Epd7In3E {
            hw: self.hw,
            state: self.state.wake_state,
        })
    }
}

impl<HW, STATE: StateAwake> Sleep<HW::Spi, HW::Error> for Epd7In3E<HW, STATE>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    type DisplayOut = Epd7In3E<HW, StateAsleep<StateUninitialized>>;

    async fn sleep(mut self, spi: &mut HW::Spi) -> Result<Self::DisplayOut, HW::Error> {
        debug!("Sleeping EPD");
        self.send(spi, Command::DeepSleep, &[0xA5]).await?;
        Ok(Epd7In3E {
            hw: self.hw,
            state: StateAsleep {
                wake_state: StateUninitialized(),
            },
        })
    }
}

impl<HW> Displayable<HW::Spi, HW::Error> for Epd7In3E<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw + DelayHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    async fn update_display(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error> {
        debug!("Updating display");
        self.send(spi, Command::PowerOn, &[]).await?;
        self.hw.wait_if_busy().await?;

        self.send(spi, Command::DisplayRefresh, &[0x00]).await?;
        self.hw.wait_if_busy().await?;

        self.send(spi, Command::PowerOff, &[0x00]).await?;
        self.hw.wait_if_busy().await?;
        Ok(())
    }
}

impl<HW> Clear<HW::Spi, HW::Error> for Epd7In3E<HW, StateReady>
where
    HW: BusyHw + DcHw + SpiHw + ErrorHw + DelayHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    async fn clear(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error> {
        let buf1_value;
        match self.state.mode {
            RefreshMode::Full => {
                buf1_value = 0xFF;
            }
        };

        self.hw
            .send_iter(
                spi,
                Command::DataStartTrans1 as u8,
                Some(core::iter::repeat_n(buf1_value, HEX_BUFFER_LENGTH)),
            )
            .await?;

        self.update_display(spi).await?;
        Ok(())
    }
}

impl<HW> DisplaySimple<1, 1, HW::Spi, HW::Error> for Epd7In3E<HW, StateReady>
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
        self.send(spi, Command::DataStartTrans1, data).await?;
        Ok(())
    }
}
