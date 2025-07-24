//! This crate provides an `async`/`await` interface for controlling Waveshare E-Paper displays.
//!
//! It is built on top of `embedded-hal-async` and `embedded-graphics`, making it compatible with a
//! wide range of embedded platforms.
//!
//! ## Core traits
//!
//! The crate is organized around two main traits:
//!
//! - [`Epd`]: This trait defines the core functionality for interacting with an E-Paper display,
//!   such as initialization, refreshing, writing image data, and managing sleep states.
//!   Implementations of this trait (e.g., [`epd2in9::Epd2In9`]) provide concrete display-specific
//!   logic. Concrete implementations may also provide further functionality that doesn't fit in
//!   the general `Epd` trait (e.g. modifying the border on the Epd2In9 screen).
//!
//! - [`EpdHw`]: This trait abstracts over the underlying hardware components required to control an
//!   E-Paper display, including SPI communication, GPIO pins (for Data/Command, Reset, and Busy
//!   signals), and a delay timer. You need to implement this trait for your chosen peripherals.
//!   This trades off some set up code (implementing this trait), for simple type signatures with
//!   only one generic parameter.
//!
//! Additionally, the crate provides:
//!
//! - `buffer` module: Contains utilities for creating and managing efficient display buffers that
//!   implement `embedded-graphics::DrawTarget`. These are designed to be fast and compact.
//! - `<display>` modules: each display lives in its own module, such as `epd2in9` for the 2.9"
//!   e-paper display.
#![no_std]

use core::error::Error as CoreError;

use embedded_graphics::{
    prelude::{DrawTarget, Point},
    primitives::Rectangle,
};
use embedded_hal::digital::{ErrorType as PinErrorType, InputPin, OutputPin};
use embedded_hal_async::{
    delay::DelayNs,
    digital::Wait,
    spi::{ErrorType as SpiErrorType, SpiDevice},
};

pub mod buffer;
pub mod epd2in9;

mod comms;
mod log;

#[allow(async_fn_in_trait)]
pub trait Epd<HW>
where
    HW: EpdHw,
{
    type RefreshMode;
    type Buffer: DrawTarget;

    /// Creates a buffer for use with this display.
    fn new_buffer(&self) -> Self::Buffer;

    fn width(&self) -> u32;

    fn height(&self) -> u32;

    /// Initialise the display. This must be called before any other operations.
    async fn init(&mut self, spi: &mut HW::Spi, mode: Self::RefreshMode) -> Result<(), HW::Error>;

    /// Sets the refresh mode for the display.
    async fn set_refresh_mode(
        &mut self,
        spi: &mut HW::Spi,
        mode: Self::RefreshMode,
    ) -> Result<(), HW::Error>;

    /// Hardware reset the display.
    async fn reset(&mut self) -> Result<(), HW::Error>;

    /// Puts the display to sleep.
    async fn sleep(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error>;

    /// Wakes and re-initialises the display (if necessary) if it's asleep.
    async fn wake(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error>;

    /// Writes the buffer's data to the display and displays it.
    async fn display_buffer(
        &mut self,
        spi: &mut HW::Spi,
        buffer: &Self::Buffer,
    ) -> Result<(), HW::Error>;

    /// Writes the buffer's data to the display's internal framebuffer, but does not display it.
    async fn write_framebuffer(
        &mut self,
        spi: &mut HW::Spi,
        buffer: &Self::Buffer,
    ) -> Result<(), HW::Error>;

    /// Sets the window to write to during a call to [Epd::write_image]. This can enable partial
    /// writes to a subsection of the display.
    async fn set_window(&mut self, spi: &mut HW::Spi, shape: Rectangle) -> Result<(), HW::Error>;

    /// Sets the cursor position for where the next byte of image data will be written.
    async fn set_cursor(
        &mut self,
        spi: &mut HW::Spi,
        position: Point,
    ) -> Result<(), <HW as EpdHw>::Error>;

    /// Writes raw image data, starting at the current cursor position and auto-incrementing x and
    /// y within the current window. By default, x should increment first, then y (data is written
    /// in rows).
    async fn write_image(&mut self, spi: &mut HW::Spi, image: &[u8]) -> Result<(), HW::Error>;

    /// Updates (refreshes) the display based on what has been written to RAM. Note that this can be
    /// stateful. For example, on the Epd2in9 display, there are two RAM buffers. Calling this
    /// function swaps the active buffer. Consider this scenario:
    ///
    /// 1. [Epd::write_image] is used to turn the RAM all white.
    /// 2. [Epd::update_display] is called, which refreshes the display to be all white.
    /// 3. [Epd::write_image] is used to turn the RAM all black.
    /// 4. [Epd::update_display] is called, which refreshes the display to be all black.
    /// 5. [Epd::update_display] is called again, which refreshes the display to be all white again.
    async fn update_display(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error>;
}

/// Provides access to the hardware needed to control an EPD.
///
/// This greatly simplifies the generics needed by the `Epd` trait and implementing types at the cost of implementing this trait.
///
/// In this example, we make the EPD generic over just the SPI type, but can drop generics for the pins and delay type.
///
/// ```rust
/// use core::convert::Infallible;
///
/// use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
/// use embassy_embedded_hal::shared_bus::SpiDeviceError;
/// use embassy_rp::gpio::{Input, Output};
/// use embassy_rp::spi::{self, Spi};
/// use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
/// use embassy_time::Delay;
/// use epd_waveshare_async::EpdHw;
/// use thiserror::Error as ThisError;
///
/// /// Define an error type that can convert from the SPI and GPIO errors.
/// #[derive(Debug, ThisError)]
/// enum Error {
///   #[error("SPI error: {0:?}")]
///   SpiError(SpiDeviceError<spi::Error, Infallible>),
/// }
///
/// impl From<Infallible> for Error {
///     fn from(_: Infallible) -> Self {
///         // GPIO errors are infallible, i.e. they can't occur, so this should be unreachable.
///         unreachable!()
///     }
/// }
///
/// impl From<SpiDeviceError<spi::Error, Infallible>> for Error {
///     fn from(e: SpiDeviceError<spi::Error, Infallible>) -> Self {
///         Error::SpiError(e)
///     }
/// }
///
/// struct RpEpdHw<'a, SPI: spi::Instance + 'a> {
///     dc: Output<'a>,
///     reset: Output<'a>,
///     busy: Input<'a>,
///     delay: Delay,
///     _phantom: core::marker::PhantomData<SPI>,
/// }
///
/// impl <'a, SPI: spi::Instance + 'a> EpdHw for RpEpdHw<'a, SPI> {
///     type Spi = SpiDevice<'a, CriticalSectionRawMutex, Spi<'a, SPI, spi::Async>, Output<'a>>;
///     type Dc = Output<'a>;
///     type Reset = Output<'a>;
///     type Busy = Input<'a>;
///     type Delay = Delay;
///     type Error = Error;
///
///     fn dc(&mut self) -> &mut Self::Dc {
///       &mut self.dc
///     }
///
///     fn reset(&mut self) -> &mut Self::Reset {
///       &mut self.reset
///     }
///
///     fn busy(&mut self) -> &mut Self::Busy {
///       &mut self.busy
///     }
///
///     fn delay(&mut self) -> &mut Self::Delay {
///       &mut self.delay
///     }
/// }
/// ```
pub trait EpdHw {
    type Spi: SpiDevice;
    type Dc: OutputPin;
    type Reset: OutputPin;
    type Busy: InputPin + Wait;
    type Delay: DelayNs;
    type Error: CoreError
        + From<<Self::Spi as SpiErrorType>::Error>
        + From<<Self::Dc as PinErrorType>::Error>
        + From<<Self::Reset as PinErrorType>::Error>
        + From<<Self::Busy as PinErrorType>::Error>;

    fn dc(&mut self) -> &mut Self::Dc;
    fn reset(&mut self) -> &mut Self::Reset;
    fn busy(&mut self) -> &mut Self::Busy;
    fn delay(&mut self) -> &mut Self::Delay;
}
