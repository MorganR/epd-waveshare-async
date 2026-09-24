//! Driver for the Waveshare 3.5" e-Paper (G): 184x384 pixels in black, white, yellow and red.
//!
//! The panel uses a Fitipower JD79667 controller. Sources used to write this driver:
//!
//! * [datasheet](https://files.waveshare.com/wiki/3.5inch_e-Paper_Module_G/3.5inch_e-Paper_(G).pdf),
//!   in particular the command table (section 7) and reference program (section 10.2).
//! * [sample code](https://github.com/waveshareteam/e-Paper/blob/master/E-paper_Separate_Program/3in5_e-Paper_G/STM32-F103ZET6/User/e-Paper/EPD_3in5g.c).
//!
//! Where the two disagree, see [InitSequence].
//!
//! Things to know about this controller:
//!
//! * BUSY is active low (the datasheet calls it BUSY_N); see [DEFAULT_BUSY_WHEN]. The controller
//!   ignores commands while busy.
//! * Waveforms come from the controller's OTP memory. There is no LUT upload and no partial
//!   refresh; every update is a full refresh.
//! * Deep sleep loses the register configuration, so waking requires a reset and full init.
//! * The Waveshare module has no MISO line, so the driver never reads from the controller.
//!
//! # Looking after the panel
//!
//! Sources: the datasheet (sections 6.1, 6.2, 11 and 14) and the precautions on Waveshare's
//! [module wiki](https://www.waveshare.com/wiki/3.5inch_e-Paper_Module_(G)_Manual).
//!
//! | Guideline | Value | Source |
//! |---|---|---|
//! | Time between refreshes | At least 180 s ([RECOMMENDED_MIN_REFRESH_INTERVAL]) | Wiki |
//! | Longest gap between refreshes while in use | 24 h ([RECOMMENDED_MAX_REFRESH_INTERVAL]) | Datasheet 14(5), wiki |
//! | Full refresh time | About 18 s at 23 °C | Datasheet 6.2 |
//! | Refresh lifetime | About 1,000,000 refreshes (Waveshare's general figure) | Wiki |
//! | Operating temperature | 0 to 40 °C | Datasheet 6.1 |
//! | Storage temperature | -25 to 70 °C, at most 10 days at the extremes | Datasheet 6.1 |
//! | Storage humidity | 55 ± 10 %RH | Datasheet 6.1 |
//!
//! * **Flicker is normal.** A full refresh drives the pigments back and forth through several
//!   colours before settling. The datasheet's inspection standards allow it.
//! * **Don't leave the panel powered.** High voltage held on the panel damages it permanently.
//!   The driver powers off after every refresh; call [Sleep::sleep] when you won't refresh for a
//!   while.
//! * **Store it white.** Before a long period without refreshes, or before removing power, call
//!   [Epd3In5G::shutdown]. It leaves the panel white, powered off and asleep. The datasheet's
//!   storage and reliability tests all use a white image.
//! * **Don't disconnect it mid-refresh.** Remove power only once [Epd3In5G::shutdown] or
//!   [Sleep::sleep] has returned.
//! * **Keep it out of sunlight.** UV permanently degrades the pigments, and heat, humidity and
//!   fluorescent light also age the panel. It's rated for indoor use.
//! * **Warm it up after the cold.** Colours can come out wrong after cold storage. Waveshare
//!   suggests leaving the panel at about 25 °C for 6 hours before refreshing.
//!
//! The 180 s minimum is the constraint that bites in practice. Refreshing every 180 s adds up to
//! 480 refreshes a day, which would reach a million refreshes in about 5.7 years. At one refresh
//! an hour, the refresh count stops being a concern, and sunlight and temperature matter more.
//! For comparison, the datasheet's 500-hour reliability tests cycle black, white, red and yellow
//! every 150 s.

use core::time::Duration;

use embedded_graphics::{
    pixelcolor::{
        raw::{RawData, RawU2},
        PixelColor,
    },
    prelude::{Point, Size},
    primitives::Rectangle,
};
use embedded_hal::{
    digital::{OutputPin, PinState},
    spi::{Phase, Polarity},
};
use embedded_hal_async::{delay::DelayNs, spi::SpiDevice};

use crate::{
    buffer::{two_bit_buffer_length, BufferView, TwoBitBuffer},
    hw::{BusyHw, BusyWait, CommandDataSend, DcHw, DelayHw, ErrorHw, ResetHw, SpiHw},
    log::debug,
    Clear, DisplaySimple, Displayable, Reset, Sleep, Wake,
};

/// The four colours the panel can show. The discriminants are the 2-bit codes sent to the
/// controller (see `EPD_3IN5G_BLACK` etc. in Waveshare's `EPD_3in5g.h`).
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Black = 0b00,
    White = 0b01,
    Yellow = 0b10,
    Red = 0b11,
}

impl PixelColor for Color {
    type Raw = RawU2;
}

impl From<RawU2> for Color {
    fn from(data: RawU2) -> Self {
        match data.into_inner() {
            0b00 => Color::Black,
            0b01 => Color::White,
            0b10 => Color::Yellow,
            _ => Color::Red,
        }
    }
}

impl From<Color> for RawU2 {
    fn from(color: Color) -> Self {
        RawU2::new(color as u8)
    }
}

/// The width of the display (portrait orientation).
pub const DISPLAY_WIDTH: u32 = 184;
/// The height of the display (portrait orientation).
pub const DISPLAY_HEIGHT: u32 = 384;
/// Leave at least this long between refreshes (Waveshare's module wiki). See the module docs for
/// the full usage guidance.
pub const RECOMMENDED_MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(180);
/// Refresh at least this often while the display is in use. Longer gaps risk ghosting and image
/// sticking (datasheet section 14, precaution 5). Before a long period without refreshes, use
/// [Epd3In5G::shutdown].
pub const RECOMMENDED_MAX_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
pub const RECOMMENDED_SPI_HZ: u32 = 4_000_000; // 4 MHz
/// Use this phase in conjunction with [RECOMMENDED_SPI_POLARITY] so that the EPD can capture data
/// on the rising edge.
pub const RECOMMENDED_SPI_PHASE: Phase = Phase::CaptureOnFirstTransition;
/// Use this polarity in conjunction with [RECOMMENDED_SPI_PHASE] so that the EPD can capture data
/// on the rising edge.
pub const RECOMMENDED_SPI_POLARITY: Polarity = Polarity::IdleLow;
/// The pin state that indicates the display is busy. The datasheet calls the pin BUSY_N.
pub const DEFAULT_BUSY_WHEN: PinState = PinState::Low;

/// The length of the underlying buffer used by [Epd3In5G].
pub const BUFFER_LENGTH: usize = two_bit_buffer_length(Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT));
/// The buffer type used by [Epd3In5G].
pub type Epd3In5GBuffer = TwoBitBuffer<BUFFER_LENGTH, Color>;
/// Constructs a new buffer for use with the [Epd3In5G] display. Every pixel starts black (code 0);
/// clear it to [Color::White] for a white background.
///
/// The buffer is 17,664 bytes. Being `const`, this lets it live in a static (e.g. a
/// `static_cell::ConstStaticCell`) without being built on the stack first.
pub const fn new_buffer() -> Epd3In5GBuffer {
    Epd3In5GBuffer::new(Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT))
}

/// Low-level commands for the JD79667. You probably want to use the other methods exposed on
/// [Epd3In5G] for most operations, but can send commands directly with [Epd3In5G::send] for
/// experimentation.
///
/// Names in brackets are the datasheet's abbreviations.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// (PSR) Resolution, scan direction, and what happens to VCOM after a refresh.
    PanelSetting = 0x00,
    /// (PWR) Charge pump voltages.
    PowerSetting = 0x01,
    /// (POF) Turns off the charge pump, keeping registers and SRAM. BUSY goes low until done.
    PowerOff = 0x02,
    /// (PFS) Power off sequence timing.
    PowerOffSequenceSetting = 0x03,
    /// (PON) Turns on the charge pump. BUSY goes low until done.
    PowerOn = 0x04,
    /// (BTST) Booster soft start.
    BoosterSoftStart = 0x06,
    /// (DSLP) Deep sleep. Takes the check code `0xA5`. Only a hardware reset wakes the chip.
    DeepSleep = 0x07,
    /// (DTM) Starts the transfer of pixel data into SRAM.
    DataStartTransmission = 0x10,
    /// (DSP) Ends the transfer and returns a data flag. Needs a read, so this driver skips it.
    DataStop = 0x11,
    /// (DRF) Refreshes the panel from SRAM. BUSY goes low until done.
    DisplayRefresh = 0x12,
    /// (PLL) Frame rate.
    PllControl = 0x30,
    /// (CDI) VCOM to data interval, data polarity and border colour. See [Epd3In5G::set_border].
    VcomDataIntervalSetting = 0x50,
    /// (TCON) Gate/source non-overlap period. Sent by the C driver, not in the datasheet.
    TconSetting = 0x60,
    /// (TRES) Resolution.
    ResolutionSetting = 0x61,
    /// (PTL) Partial window. Unused: this panel has no partial refresh waveform.
    PartialWindow = 0x83,
    /// (PWS) Power saving.
    PowerSaving = 0xE3,
    /// Vendor key sent first by the C driver. The datasheet's reference flow is headed "Enter
    /// FITI Command" but doesn't show this command, so it's probably what unlocks the
    /// undocumented registers below.
    VendorKey = 0x66,
    /// Undocumented. In both the datasheet flow and the C driver.
    Undocumented4D = 0x4D,
    /// Undocumented. In both the datasheet flow and the C driver.
    UndocumentedB4 = 0xB4,
    /// Undocumented. In both the datasheet flow and the C driver.
    UndocumentedB6 = 0xB6,
    /// Undocumented. C driver only.
    UndocumentedE7 = 0xE7,
    /// Undocumented. In both the datasheet flow and the C driver.
    UndocumentedE9 = 0xE9,
}

impl Command {
    /// Returns the register address for this command.
    fn register(&self) -> u8 {
        *self as u8
    }
}

/// `(DISPLAY_WIDTH, DISPLAY_HEIGHT)` as sent with [Command::ResolutionSetting].
const RESOLUTION_DATA: [u8; 4] = [
    (DISPLAY_WIDTH >> 8) as u8,
    DISPLAY_WIDTH as u8,
    (DISPLAY_HEIGHT >> 8) as u8,
    DISPLAY_HEIGHT as u8,
];

/// Register writes sent by Waveshare's C driver (`EPD_3IN5G_Init` in `EPD_3in5g.c`), in order.
///
/// The V2 panel's C driver (`EPD_3in5g_V2.c`) uses the same sequence.
const VENDOR_INIT: &[(Command, &[u8])] = &[
    (Command::VendorKey, &[0x49, 0x55, 0x13, 0x5D, 0x05, 0x10]),
    (Command::Undocumented4D, &[0x78]),
    (Command::PanelSetting, &[0x0F, 0x29]),
    (Command::PowerSetting, &[0x07, 0x00]),
    (Command::PowerOffSequenceSetting, &[0x10, 0x54, 0x44]),
    (
        Command::BoosterSoftStart,
        &[0x0F, 0x0A, 0x2F, 0x25, 0x22, 0x2E, 0x21],
    ),
    (Command::VcomDataIntervalSetting, &[DEFAULT_CDI]),
    (Command::TconSetting, &[0x02, 0x02]),
    (Command::ResolutionSetting, &RESOLUTION_DATA),
    (Command::UndocumentedE7, &[0x1C]),
    (Command::PowerSaving, &[0x22]),
    (Command::UndocumentedB6, &[0x6F]),
    (Command::UndocumentedB4, &[0xD0]),
    (Command::UndocumentedE9, &[0x01]),
    (Command::PllControl, &[0x08]),
];

/// Register writes from the datasheet's reference program (section 10.2), in order.
const DATASHEET_INIT: &[(Command, &[u8])] = &[
    (Command::Undocumented4D, &[0x78]),
    (Command::PanelSetting, &[0x0F, 0x29]),
    (Command::PowerOffSequenceSetting, &[0x10, 0x54, 0x44]),
    (
        Command::BoosterSoftStart,
        &[0x0F, 0x0A, 0x2F, 0x25, 0x22, 0x2E, 0x21],
    ),
    (Command::PllControl, &[0x08]),
    (Command::VcomDataIntervalSetting, &[DEFAULT_CDI]),
    (Command::ResolutionSetting, &RESOLUTION_DATA),
    (Command::UndocumentedB4, &[0xD0]),
    (Command::UndocumentedB6, &[0x6F]),
    (Command::PowerSaving, &[0x22]),
    (Command::UndocumentedE9, &[0x01]),
];

/// Which register sequence [Epd3In5G::init] sends.
///
/// The datasheet's reference program (section 10.2) and Waveshare's C driver agree on every
/// register they both write, but the C driver also sends [Command::VendorKey],
/// [Command::PowerSetting], [Command::TconSetting] and [Command::UndocumentedE7], and orders the
/// commands differently. The vendor sequence is the one tested on hardware.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InitSequence {
    /// The sequence from Waveshare's C driver.
    #[default]
    Vendor,
    /// The shorter sequence from the datasheet's reference program.
    Datasheet,
}

impl InitSequence {
    fn commands(&self) -> &'static [(Command, &'static [u8])] {
        match self {
            InitSequence::Vendor => VENDOR_INIT,
            InitSequence::Datasheet => DATASHEET_INIT,
        }
    }
}

/// CDI value sent by both init sequences: 10 hsync VCOM-to-data interval (`0x7`), DDX set so
/// pixel codes map straight to [Color], and a white border.
const DEFAULT_CDI: u8 = cdi(Color::White);
const DEFAULT_BORDER: Color = Color::White;

/// Builds the [Command::VcomDataIntervalSetting] byte. With DDX=1 the border bits (VBD, bits
/// 7-5) take the same codes as pixels; see the CDI table in the datasheet.
const fn cdi(border: Color) -> u8 {
    const DDX: u8 = 1 << 4;
    const INTERVAL_10_HSYNC: u8 = 0x7;
    ((border as u8) << 5) | DDX | INTERVAL_10_HSYNC
}

trait StateInternal {}
#[allow(private_bounds)]
pub trait State: StateInternal {}

/// The display has not been initialised since power up or the last reset.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateUninitialized();
impl StateInternal for StateUninitialized {}
impl State for StateUninitialized {}

/// The display is initialised and accepts image data.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateReady {
    sequence: InitSequence,
    border: Color,
    /// Whether the charge pump is on ([Command::PowerOn] sent without a later
    /// [Command::PowerOff]).
    powered: bool,
}
impl StateInternal for StateReady {}
impl State for StateReady {}

/// The display is in deep sleep. Waking it resets and re-initialises it with the settings it
/// had before sleeping.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateAsleep {
    ready: StateReady,
}
impl StateInternal for StateAsleep {}
impl State for StateAsleep {}

/// Controls the Waveshare 3.5" e-Paper (G) display.
///
/// The display has a portrait orientation, and uses [Color] for its four colours.
///
/// HW should implement [ResetHw], [BusyHw], [DcHw], [SpiHw], [DelayHw], and [ErrorHw], with
/// [BusyHw::busy_when] returning [DEFAULT_BUSY_WHEN].
///
/// A typical update:
///
/// 1. [Epd3In5G::new], then [Epd3In5G::init].
/// 2. Draw into a buffer from [new_buffer].
/// 3. [DisplaySimple::display_framebuffer]. This powers the panel on, refreshes it, and
///    powers it off again, returning once the refresh finishes (about 18 seconds).
/// 4. [Sleep::sleep] when you won't update the display for a while, or [Epd3In5G::shutdown]
///    before removing power.
pub struct Epd3In5G<HW, STATE> {
    hw: HW,
    state: STATE,
}

impl<HW, STATE> Epd3In5G<HW, STATE>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
    STATE: State,
{
    /// Waits until the display is idle.
    async fn wait_until_idle(&mut self) -> Result<(), HW::Error> {
        // Give the controller time to pull BUSY low after the previous command.
        self.hw.delay().delay_ms(1).await;
        self.hw.wait_if_busy().await
    }

    /// Sends the command and data. Waits until the display is idle before sending.
    async fn send_impl(
        &mut self,
        spi: &mut HW::Spi,
        command: Command,
        data: &[u8],
    ) -> Result<(), HW::Error> {
        self.hw.send(spi, command.register(), data).await
    }

    async fn reset_impl(&mut self) -> Result<(), HW::Error> {
        debug!("Resetting EPD");
        // Timings from the C driver.
        self.hw.reset().set_high()?;
        self.hw.delay().delay_ms(200).await;
        self.hw.reset().set_low()?;
        self.hw.delay().delay_ms(2).await;
        self.hw.reset().set_high()?;
        self.hw.delay().delay_ms(200).await;
        Ok(())
    }

    /// Resets the display and sends the given init sequence.
    async fn init_impl(
        mut self,
        spi: &mut HW::Spi,
        ready: StateReady,
    ) -> Result<Epd3In5G<HW, StateReady>, HW::Error> {
        self.reset_impl().await?;
        for (command, data) in ready.sequence.commands() {
            self.send_impl(spi, *command, data).await?;
        }
        let mut epd = Epd3In5G {
            hw: self.hw,
            state: StateReady {
                powered: false,
                ..ready
            },
        };
        if ready.border != DEFAULT_BORDER {
            epd.set_border(spi, ready.border).await?;
        }
        Ok(epd)
    }
}

impl<HW> Epd3In5G<HW, StateUninitialized>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    pub fn new(hw: HW) -> Self {
        Epd3In5G {
            hw,
            state: StateUninitialized(),
        }
    }

    /// Resets and initialises the display. [InitSequence::default] is the right choice unless
    /// you're comparing sequences.
    pub async fn init(
        self,
        spi: &mut HW::Spi,
        sequence: InitSequence,
    ) -> Result<Epd3In5G<HW, StateReady>, HW::Error> {
        debug!("Initialising display with {:?} sequence", sequence);
        self.init_impl(
            spi,
            StateReady {
                sequence,
                border: DEFAULT_BORDER,
                powered: false,
            },
        )
        .await
    }
}

impl<HW> Epd3In5G<HW, StateReady>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    /// Sends a raw command and data. Waits until the display is idle before sending.
    pub async fn send(
        &mut self,
        spi: &mut HW::Spi,
        command: Command,
        data: &[u8],
    ) -> Result<(), HW::Error> {
        self.send_impl(spi, command, data).await
    }

    /// Sets the border colour. Takes effect on the next refresh.
    pub async fn set_border(&mut self, spi: &mut HW::Spi, color: Color) -> Result<(), HW::Error> {
        debug!("Setting border to {:?}", color);
        self.state.border = color;
        self.send_impl(spi, Command::VcomDataIntervalSetting, &[cdi(color)])
            .await
    }

    /// Fills the whole display with one colour and refreshes it. Streams the colour code
    /// directly, so no framebuffer is needed.
    pub async fn fill(&mut self, spi: &mut HW::Spi, color: Color) -> Result<(), HW::Error> {
        debug!("Filling display with {:?}", color);
        const CHUNK_LENGTH: usize = 64;
        const { assert!(BUFFER_LENGTH.is_multiple_of(CHUNK_LENGTH)) };
        let chunk = [(color as u8) * 0b0101_0101; CHUNK_LENGTH];

        self.power_on(spi).await?;
        self.send_impl(spi, Command::DataStartTransmission, &[])
            .await?;
        self.hw.dc().set_high()?;
        for _ in 0..BUFFER_LENGTH / CHUNK_LENGTH {
            spi.write(&chunk).await?;
        }
        self.update_display(spi).await
    }

    /// Leaves the display the way the datasheet says to keep it when unused for long periods:
    /// showing white with a white border (sections 6.1, 11 and 14), with the charge pump off and
    /// the controller in deep sleep.
    ///
    /// Once this returns, it's safe to remove power. Section 14 warns against disconnecting the
    /// panel while it's operating.
    pub async fn shutdown(
        mut self,
        spi: &mut HW::Spi,
    ) -> Result<Epd3In5G<HW, StateAsleep>, HW::Error> {
        debug!("Shutting down EPD");
        self.set_border(spi, Color::White).await?;
        self.fill(spi, Color::White).await?;
        self.sleep(spi).await
    }

    async fn power_on(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error> {
        if !self.state.powered {
            self.send_impl(spi, Command::PowerOn, &[]).await?;
            self.wait_until_idle().await?;
            self.state.powered = true;
        }
        Ok(())
    }

    async fn power_off(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error> {
        if self.state.powered {
            self.send_impl(spi, Command::PowerOff, &[0x00]).await?;
            self.wait_until_idle().await?;
            self.state.powered = false;
        }
        Ok(())
    }
}

impl<HW> Displayable<HW::Spi, HW::Error> for Epd3In5G<HW, StateReady>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    /// Refreshes the panel from the image in SRAM, then powers the panel off. Returns once the
    /// refresh has finished.
    ///
    /// Powering off after each refresh follows the datasheet's reference program. The C driver
    /// leaves the power on until sleep, but Waveshare warns that holding the panel at high
    /// voltage for long periods damages it.
    async fn update_display(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error> {
        debug!("Refreshing display");
        self.power_on(spi).await?;
        // 0x00: VCOM follows the LUT during the refresh (AC VCOM, the default).
        self.send_impl(spi, Command::DisplayRefresh, &[0x00])
            .await?;
        self.wait_until_idle().await?;
        self.power_off(spi).await
    }
}

impl<HW> Clear<HW::Spi, HW::Error> for Epd3In5G<HW, StateReady>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    /// Fills the display with white and refreshes it.
    async fn clear(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error> {
        self.fill(spi, Color::White).await
    }
}

impl<HW> DisplaySimple<2, 1, HW::Spi, HW::Error> for Epd3In5G<HW, StateReady>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    async fn display_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buf: &dyn BufferView<2, 1>,
    ) -> Result<(), HW::Error> {
        self.write_framebuffer(spi, buf).await?;
        self.update_display(spi).await
    }

    /// Writes the whole image to SRAM. The buffer must cover the full display, since this
    /// panel has no partial refresh.
    async fn write_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buf: &dyn BufferView<2, 1>,
    ) -> Result<(), HW::Error> {
        let full_screen = Rectangle::new(Point::zero(), Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT));
        debug_assert!(
            buf.window() == full_screen && buf.data()[0].len() == BUFFER_LENGTH,
            "buffer must cover the whole display"
        );
        // The datasheet's reference program powers on before sending data.
        self.power_on(spi).await?;
        self.send_impl(spi, Command::DataStartTransmission, buf.data()[0])
            .await
    }
}

impl<HW, STATE> Reset<HW::Error> for Epd3In5G<HW, STATE>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
    STATE: State,
{
    /// A hardware reset clears the register configuration, so the display must be initialised
    /// again afterwards.
    type DisplayOut = Epd3In5G<HW, StateUninitialized>;

    async fn reset(mut self) -> Result<Self::DisplayOut, HW::Error> {
        self.reset_impl().await?;
        Ok(Epd3In5G {
            hw: self.hw,
            state: StateUninitialized(),
        })
    }
}

impl<HW> Sleep<HW::Spi, HW::Error> for Epd3In5G<HW, StateReady>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    type DisplayOut = Epd3In5G<HW, StateAsleep>;

    /// Powers off the panel if needed, then enters deep sleep. The image stays on the panel.
    ///
    /// Use [Epd3In5G::shutdown] instead before a long period without refreshes.
    async fn sleep(mut self, spi: &mut HW::Spi) -> Result<Self::DisplayOut, HW::Error> {
        debug!("Sleeping EPD");
        self.power_off(spi).await?;
        self.send_impl(spi, Command::DeepSleep, &[0xA5]).await?;
        Ok(Epd3In5G {
            hw: self.hw,
            state: StateAsleep { ready: self.state },
        })
    }
}

impl<HW> Wake<HW::Spi, HW::Error> for Epd3In5G<HW, StateAsleep>
where
    HW: BusyHw + DcHw + ResetHw + DelayHw + SpiHw + ErrorHw,
    HW::Error: From<<HW::Busy as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Dc as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Reset as embedded_hal::digital::ErrorType>::Error>
        + From<<HW::Spi as embedded_hal_async::spi::ErrorType>::Error>,
{
    type DisplayOut = Epd3In5G<HW, StateReady>;

    /// Resets the display and re-runs the init sequence it used before sleeping.
    async fn wake(self, spi: &mut HW::Spi) -> Result<Self::DisplayOut, HW::Error> {
        debug!("Waking EPD");
        let ready = self.state.ready;
        self.init_impl(spi, ready).await
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::vec;
    use std::vec::Vec;

    use embedded_graphics::prelude::*;

    use super::*;
    use crate::mock::{block_on, Event, Log, MockHw, MockSpi};

    /// Commands that make the JD79667 pull BUSY low until they finish.
    const BUSY_COMMANDS: &[u8] = &[0x04, 0x12, 0x02];

    /// `EPD_3IN5G_Init` from Waveshare's `EPD_3in5g.c`, transcribed independently of
    /// [VENDOR_INIT].
    fn c_driver_init() -> Vec<(u8, Vec<u8>)> {
        vec![
            (0x66, vec![0x49, 0x55, 0x13, 0x5D, 0x05, 0x10]),
            (0x4D, vec![0x78]),
            (0x00, vec![0x0F, 0x29]),
            (0x01, vec![0x07, 0x00]),
            (0x03, vec![0x10, 0x54, 0x44]),
            (0x06, vec![0x0F, 0x0A, 0x2F, 0x25, 0x22, 0x2E, 0x21]),
            (0x50, vec![0x37]),
            (0x60, vec![0x02, 0x02]),
            // EPD_3IN5G_WIDTH / 256, WIDTH % 256, HEIGHT / 256, HEIGHT % 256
            (0x61, vec![0x00, 0xB8, 0x01, 0x80]),
            (0xE7, vec![0x1C]),
            (0xE3, vec![0x22]),
            (0xB6, vec![0x6F]),
            (0xB4, vec![0xD0]),
            (0xE9, vec![0x01]),
            (0x30, vec![0x08]),
        ]
    }

    /// Section 10.2 of the datasheet, transcribed independently of [DATASHEET_INIT].
    fn datasheet_init() -> Vec<(u8, Vec<u8>)> {
        vec![
            (0x4D, vec![0x78]),
            (0x00, vec![0x0F, 0x29]),
            (0x03, vec![0x10, 0x54, 0x44]),
            (0x06, vec![0x0F, 0x0A, 0x2F, 0x25, 0x22, 0x2E, 0x21]),
            (0x30, vec![0x08]),
            (0x50, vec![0x37]),
            (0x61, vec![0x00, 0xB8, 0x01, 0x80]),
            (0xB4, vec![0xD0]),
            (0xB6, vec![0x6F]),
            (0xE3, vec![0x22]),
            (0xE9, vec![0x01]),
        ]
    }

    fn ready_display(sequence: InitSequence) -> (Epd3In5G<MockHw, StateReady>, MockSpi, Log) {
        let (hw, mut spi, log) = MockHw::new(DEFAULT_BUSY_WHEN, BUSY_COMMANDS);
        let epd = block_on(Epd3In5G::new(hw).init(&mut spi, sequence)).unwrap();
        (epd, spi, log)
    }

    fn command_names(log: &Log) -> Vec<u8> {
        log.commands_with_data().iter().map(|(c, _)| *c).collect()
    }

    fn white_buffer() -> Epd3In5GBuffer {
        let mut buffer = new_buffer();
        buffer.clear(Color::White).unwrap();
        buffer
    }

    #[test]
    fn init_matches_c_driver() {
        let (_epd, _spi, log) = ready_display(InitSequence::Vendor);
        assert_eq!(log.commands_with_data(), c_driver_init());
        assert!(log.violations().is_empty(), "{:?}", log.violations());
    }

    #[test]
    fn init_matches_datasheet() {
        let (_epd, _spi, log) = ready_display(InitSequence::Datasheet);
        assert_eq!(log.commands_with_data(), datasheet_init());
    }

    #[test]
    fn init_pulses_reset_low() {
        let (_epd, _spi, log) = ready_display(InitSequence::Vendor);
        let events = log.events();
        let resets: Vec<PinState> = events
            .iter()
            .filter_map(|e| match e {
                Event::Reset(level) => Some(*level),
                _ => None,
            })
            .collect();
        assert_eq!(resets, vec![PinState::High, PinState::Low, PinState::High]);
        // Reset must finish before the first command.
        let first_command = events
            .iter()
            .position(|e| matches!(e, Event::Command(_)))
            .unwrap();
        let last_reset = events
            .iter()
            .rposition(|e| matches!(e, Event::Reset(_)))
            .unwrap();
        assert!(last_reset < first_command);
    }

    #[test]
    fn display_follows_datasheet_flow() {
        let (mut epd, mut spi, log) = ready_display(InitSequence::Vendor);
        log.clear();

        block_on(epd.display_framebuffer(&mut spi, &white_buffer())).unwrap();

        // Section 10.2: PON, wait, DTM + data, DRF, wait, POF.
        let commands = log.commands_with_data();
        assert_eq!(command_names(&log), vec![0x04, 0x10, 0x12, 0x02]);
        assert_eq!(commands[1].1.len(), BUFFER_LENGTH);
        assert_eq!(commands[2].1, vec![0x00]);
        assert!(log.violations().is_empty(), "{:?}", log.violations());
        // The refresh must have finished by the time display_framebuffer returns.
        assert!(!log.is_busy());
    }

    #[test]
    fn white_buffer_sends_white_codes() {
        let (mut epd, mut spi, log) = ready_display(InitSequence::Vendor);
        log.clear();
        block_on(epd.display_framebuffer(&mut spi, &white_buffer())).unwrap();
        let data = &log.commands_with_data()[1].1;
        // `EPD_3IN5G_Clear(EPD_3IN5G_WHITE)` sends 0b01 in each 2-bit slot.
        assert!(data.iter().all(|b| *b == 0x55));
    }

    #[test]
    fn colour_bands_pack_as_c_driver_expects() {
        let (mut epd, mut spi, log) = ready_display(InitSequence::Vendor);
        log.clear();

        let mut buffer = new_buffer();
        let band_height = DISPLAY_HEIGHT / 4;
        for (i, color) in [Color::Black, Color::White, Color::Yellow, Color::Red]
            .into_iter()
            .enumerate()
        {
            buffer
                .fill_solid(
                    &Rectangle::new(
                        Point::new(0, (i as u32 * band_height) as i32),
                        Size::new(DISPLAY_WIDTH, band_height),
                    ),
                    color,
                )
                .unwrap();
        }
        block_on(epd.display_framebuffer(&mut spi, &buffer)).unwrap();

        let data = &log.commands_with_data()[1].1;
        let bytes_per_row = DISPLAY_WIDTH as usize / 4;
        let band_bytes = bytes_per_row * band_height as usize;
        // (color << 6) | (color << 4) | (color << 2) | color, as in EPD_3IN5G_Clear.
        for (band, expected) in [0x00u8, 0x55, 0xAA, 0xFF].into_iter().enumerate() {
            let slice = &data[band * band_bytes..(band + 1) * band_bytes];
            assert!(
                slice.iter().all(|b| *b == expected),
                "band {band} expected {expected:#04x}"
            );
        }
    }

    #[test]
    fn sleep_powers_off_then_sends_check_code() {
        let (mut epd, mut spi, log) = ready_display(InitSequence::Vendor);
        block_on(epd.write_framebuffer(&mut spi, &white_buffer())).unwrap();
        log.clear();

        let _epd = block_on(epd.sleep(&mut spi)).unwrap();

        assert_eq!(
            log.commands_with_data(),
            vec![(0x02, vec![0x00]), (0x07, vec![0xA5])]
        );
        assert!(log.violations().is_empty(), "{:?}", log.violations());
    }

    #[test]
    fn sleep_after_refresh_skips_power_off() {
        let (mut epd, mut spi, log) = ready_display(InitSequence::Vendor);
        block_on(epd.display_framebuffer(&mut spi, &white_buffer())).unwrap();
        log.clear();

        let _epd = block_on(epd.sleep(&mut spi)).unwrap();

        assert_eq!(log.commands_with_data(), vec![(0x07, vec![0xA5])]);
    }

    #[test]
    fn wake_resets_and_reinitialises() {
        let (mut epd, mut spi, log) = ready_display(InitSequence::Datasheet);
        block_on(epd.set_border(&mut spi, Color::Red)).unwrap();
        let epd = block_on(epd.sleep(&mut spi)).unwrap();
        log.clear();

        let _epd = block_on(epd.wake(&mut spi)).unwrap();

        assert!(log.events().contains(&Event::Reset(PinState::Low)));
        let mut expected = datasheet_init();
        // Border set before sleeping: 0b011 (red) in bits 7-5, DDX, 10 hsync.
        expected.push((0x50, vec![0x77]));
        assert_eq!(log.commands_with_data(), expected);
    }

    #[test]
    fn fill_streams_one_colour_without_a_buffer() {
        let (mut epd, mut spi, log) = ready_display(InitSequence::Vendor);
        log.clear();

        block_on(epd.fill(&mut spi, Color::Yellow)).unwrap();

        let commands = log.commands_with_data();
        assert_eq!(command_names(&log), vec![0x04, 0x10, 0x12, 0x02]);
        assert_eq!(commands[1].1, vec![0xAA; BUFFER_LENGTH]);
        assert!(log.violations().is_empty(), "{:?}", log.violations());
    }

    #[test]
    fn clear_fills_white() {
        let (mut epd, mut spi, log) = ready_display(InitSequence::Vendor);
        log.clear();

        block_on(Clear::clear(&mut epd, &mut spi)).unwrap();

        assert_eq!(log.commands_with_data()[1].1, vec![0x55; BUFFER_LENGTH]);
    }

    #[test]
    fn shutdown_leaves_white_image_and_sleeps() {
        let (mut epd, mut spi, log) = ready_display(InitSequence::Vendor);
        block_on(epd.set_border(&mut spi, Color::Red)).unwrap();
        log.clear();

        let _epd = block_on(epd.shutdown(&mut spi)).unwrap();

        let commands = log.commands_with_data();
        // White border, PON, white data, DRF, POF, deep sleep.
        assert_eq!(
            command_names(&log),
            vec![0x50, 0x04, 0x10, 0x12, 0x02, 0x07]
        );
        assert_eq!(commands[0].1, vec![0x37]);
        assert_eq!(commands[2].1, vec![0x55; BUFFER_LENGTH]);
        assert_eq!(commands[5].1, vec![0xA5]);
        assert!(log.violations().is_empty(), "{:?}", log.violations());
        // The refresh and power off must have finished before the deep sleep command.
        let events = log.events();
        let sleep = events
            .iter()
            .position(|e| *e == Event::Command(0x07))
            .unwrap();
        let waits = events[..sleep]
            .iter()
            .filter(|e| **e == Event::WaitedForIdle)
            .count();
        assert!(waits >= 3, "expected waits after PON, DRF and POF");
    }

    #[test]
    fn border_codes_match_datasheet_table() {
        // CDI table: DDX=1, VBD 000..011 select Gray0..Gray3, i.e. the pixel codes.
        assert_eq!(cdi(Color::Black), 0x17);
        assert_eq!(cdi(Color::White), 0x37);
        assert_eq!(cdi(Color::Yellow), 0x57);
        assert_eq!(cdi(Color::Red), 0x77);
    }
}
