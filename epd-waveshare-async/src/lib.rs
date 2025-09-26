//! This crate provides an `async`/`await` interface for controlling Waveshare E-Paper displays.
//!
//! It is built on top of `embedded-hal-async` and `embedded-graphics`, making it compatible with a
//! wide range of embedded platforms.
//!
//! ## Core traits
//!
//! ### Hardware
//!
//! The user must implement the `XHw` traits for their hardware that are needed by their display.
//! These traits abstract over common hardware functionality that displays need, like SPI
//! communication, GPIO pins (for Data/Command, Reset, and Busy) and a delay timer. You need to
//! implement these traits for your chosen peripherals. This trades off some set up code (
//! implementing these traits), for simple type signatures with fewer generic parameters.
//!
//! See the [crate::hw] module for more.
//!
//! ### Functionality
//!
//! Functionality is split into composable traits, to enable granular support per display, and
//! stateful functionality that can be checked at compilation time.
//!
//! * [Reset]: basic hardware reset support
//! * [Sleep]: displays that can be put to sleep
//! * [Wake]: displays that can be woken from sleep
//! * [DisplaySimple]: basic support for writing and displaying a single framebuffer
//! * [DisplayPartial]: support for partial refresh using a diff
//!
//! Additionally, the crate provides:
//!
//! - [`buffer`] module: Contains utilities for creating and managing efficient display buffers that
//!   implement `embedded-graphics::DrawTarget`. These are designed to be fast and compact.
//! - various `<display>` modules: each display lives in its own module, such as `epd2in9` for the 2.9"
//!   e-paper display.
#![no_std]
#![allow(async_fn_in_trait)]

use embedded_hal_async::spi::SpiDevice;

pub mod buffer;
pub mod epd2in9;
pub mod epd2in9_v2;
/// This module provides hardware abstraction traits that can be used by display drivers.
/// You should implement all the traits on a single struct, so that you can pass this one
/// hardware struct to your display driver.
///
/// Example that remains generic over the specific SPI bus:
///
/// ```
/// # use core::convert::Infallible;
/// # use core::marker::PhantomData;
/// use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice as EmbassySpiDevice;
/// use embassy_embedded_hal::shared_bus::SpiDeviceError;
/// use embassy_rp::gpio::{Input, Level, Output, Pin, Pull};
/// use embassy_rp::spi;
/// use embassy_rp::Peri;
/// use embassy_sync::blocking_mutex::raw::NoopRawMutex;
/// use embedded_hal::digital::PinState;
/// use epd_waveshare_async::hw::{BusyHw, DcHw, DelayHw, ErrorHw, ResetHw, SpiHw};
/// use thiserror::Error as ThisError;
///
/// /// Defines the hardware to use for connecting to the display.
/// pub struct DisplayHw<'a, SPI> {
///     dc: Output<'a>,
///     reset: Output<'a>,
///     busy: Input<'a>,
///     delay: embassy_time::Delay,
///     _spi_type: PhantomData<SPI>,
/// }
///
/// impl<'a, SPI: spi::Instance> DisplayHw<'a, SPI> {
///     pub fn new<DC: Pin, RESET: Pin, BUSY: Pin>(
///         dc: Peri<'a, DC>,
///         reset: Peri<'a, RESET>,
///         busy: Peri<'a, BUSY>,
///     ) -> Self {
///         let dc = Output::new(dc, Level::High);
///         let reset = Output::new(reset, Level::High);
///         let busy = Input::new(busy, Pull::Up);
///
///         Self {
///             dc,
///             reset,
///             busy,
///             delay: embassy_time::Delay,
///             _spi_type: PhantomData,
///         }
///     }
/// }
///
/// impl<'a, SPI> ErrorHw for DisplayHw<'a, SPI> {
///     type Error = Error;
/// }
///
/// impl<'a, SPI> DcHw for DisplayHw<'a, SPI> {
///     type Dc = Output<'a>;
///
///     fn dc(&mut self) -> &mut Self::Dc {
///         &mut self.dc
///     }
/// }
///
/// impl<'a, SPI> ResetHw for DisplayHw<'a, SPI> {
///     type Reset = Output<'a>;
///
///     fn reset(&mut self) -> &mut Self::Reset {
///         &mut self.reset
///     }
/// }
///
/// impl<'a, SPI> BusyHw for DisplayHw<'a, SPI> {
///     type Busy = Input<'a>;
///
///     fn busy(&mut self) -> &mut Self::Busy {
///         &mut self.busy
///     }
///
///     fn busy_when(&self) -> embedded_hal::digital::PinState {
///         epd_waveshare_async::epd2in9::DEFAULT_BUSY_WHEN
///     }
/// }
///
/// impl<'a, SPI> DelayHw for DisplayHw<'a, SPI> {
///     type Delay = embassy_time::Delay;
///
///     fn delay(&mut self) -> &mut Self::Delay {
///         &mut self.delay
///     }
/// }
///
/// impl<'a, SPI: spi::Instance + 'a> SpiHw for DisplayHw<'a, SPI> {
///     type Spi = EmbassySpiDevice<'a, NoopRawMutex, spi::Spi<'a, SPI, spi::Async>, Output<'a>>;
/// }
///
/// type RawSpiError = SpiDeviceError<spi::Error, Infallible>;
///
/// #[derive(Debug, ThisError)]
/// pub enum Error {
///     #[error("SPI error: {0:?}")]
///     SpiError(RawSpiError),
/// }
///
/// impl From<Infallible> for Error {
///     fn from(_: Infallible) -> Self {
///         unreachable!()
///     }
/// }
///
/// impl From<RawSpiError> for Error {
///     fn from(e: RawSpiError) -> Self {
///         Error::SpiError(e)
///     }
/// }
/// ```
pub mod hw;

mod log;

use crate::buffer::BufferView;

/// Displays that have a hardware reset.
pub trait Reset<ERROR> {
    type DisplayOut;

    /// Hardware resets the display.
    async fn reset(self) -> Result<Self::DisplayOut, ERROR>;
}

/// Displays that can sleep to save power.
pub trait Sleep<SPI: SpiDevice, ERROR> {
    type DisplayOut;

    /// Puts the display to sleep.
    async fn sleep(self, spi: &mut SPI) -> Result<Self::DisplayOut, ERROR>;
}

/// Displays that can be woken from a sleep state.
pub trait Wake<SPI: SpiDevice, ERROR> {
    type DisplayOut;

    /// Wakes and re-initialises the display (if necessary) if it's asleep.
    async fn wake(self, spi: &mut SPI) -> Result<Self::DisplayOut, ERROR>;
}

/// Base trait for any display where the display can be updated separate from its framebuffer data.
pub trait Displayable<SPI: SpiDevice, ERROR> {
    /// Updates (refreshes) the display based on what has been written to the framebuffer.
    async fn update_display(&mut self, spi: &mut SPI) -> Result<(), ERROR>;
}

/// Simple displays that support writing and displaying framebuffers of a certain bit configuration.
///
/// `BITS` indicates the colour depth of each frame, and `FRAMES` indicates the total number of frames that
/// represent a complete image. For example, some 4-colour greyscale display might accept data as two
/// separate 1-bit frames instead of one frame of 2-bit pixels. This distinction is exposed so that
/// framebuffers can be written directly to displays without temp copies or transformations.
pub trait DisplaySimple<const BITS: usize, const FRAMES: usize, SPI: SpiDevice, ERROR>:
    Displayable<SPI, ERROR>
{
    /// Writes the given buffer's data into the main framebuffer to be displayed on the next call to [Displayable::update_display].
    async fn write_framebuffer(
        &mut self,
        spi: &mut SPI,
        buf: &dyn BufferView<BITS, FRAMES>,
    ) -> Result<(), ERROR>;

    /// A shortcut for calling [DisplaySimple::write_framebuffer] followed by [Displayable::update_display].
    async fn display_framebuffer(
        &mut self,
        spi: &mut SPI,
        buf: &dyn BufferView<BITS, FRAMES>,
    ) -> Result<(), ERROR>;
}

/// Displays that support a partial update, where a "diff" framebuffer is diffed against a base
/// framebuffer, and only the changed pixels from the diff are actually updated.
pub trait DisplayPartial<const BITS: usize, const FRAMES: usize, SPI: SpiDevice, ERROR>:
    DisplaySimple<BITS, FRAMES, SPI, ERROR>
{
    /// Writes the buffer to the base framebuffer that the main framebuffer layer (written with
    /// [DisplaySimple::write_framebuffer]) will be diffed against.
    /// Only pixels that differ will be updated.
    ///
    /// For standard use, you probably only need to call this once before the first partial display,
    /// as the main framebuffer becomes the diff base after a call to [Displayable::update_display].
    async fn write_base_framebuffer(
        &mut self,
        spi: &mut SPI,
        buf: &dyn BufferView<BITS, FRAMES>,
    ) -> Result<(), ERROR>;
}
