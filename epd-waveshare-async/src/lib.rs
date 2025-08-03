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
#![allow(async_fn_in_trait)]

use embedded_graphics::{
    prelude::DrawTarget,
};
use embedded_hal_async::spi::SpiDevice;

pub mod buffer;
pub mod epd2in9;
pub mod epd2in9_v2;

mod hw;
mod log;

use crate::buffer::BufferView;
pub use crate::hw::EpdHw;

/// Indicates usage errors due to incorrect states.
/// 
/// These errors are allowed to occur as runtime errors instead of being prevented at compile time
/// through stateful types. The alternative was tried, but was awkward to use in practice since
/// async traits are not dyn compatible.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone)]
pub enum Error {
    Uninitialized,
    Sleeping,
    WrongRefreshMode,
}

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
    /// Updates (refreshes) the display based on what has been written to RAM. Note that this can be
    /// stateful, for example displays in [DisplayPartial] mode often swap two underlying
    /// framebuffers on calls to this method, resulting in the following behaviour:
    ///
    /// 1. [DisplayPartial::write_diff_framebuffer] is used to turn the RAM all white.
    /// 2. [Displayable::update_display] is called, which refreshes the display to be all white.
    /// 3. [DisplayPartial::write_diff_framebuffer] is used to turn the RAM all black.
    /// 4. [Displayable::update_display] is called, which refreshes the display to be all black.
    /// 5. [Displayable::update_display] is called again, which refreshes the display to be all white again.
    async fn update_display(&mut self, spi: &mut SPI) -> Result<(), ERROR>;
}

/// Simple displays that support writing and displaying framebuffers of a certain bit configuration.
/// 
/// `BITS` indicates the colour depth of each frame, and `FRAMES` indicates the total number of frames that
/// represent a complete image. For example, some 4-colour greyscale displays accept data as two
/// separate 1-bit frames instead of one frame of 2-bit pixels. This distinction is exposed so that
/// framebuffers can be written directly to displays without temp copies or transformations.
pub trait DisplaySimple<const BITS: usize, const FRAMES: usize, SPI: SpiDevice, ERROR>: Displayable<SPI, ERROR> {
    /// Writes the given buffer's data into the main framebuffer to be displayed on the next call to [Displayable::update_display].
    async fn write_framebuffer(&mut self, spi: &mut SPI, buf: &dyn BufferView<BITS, FRAMES>) -> Result<(), ERROR>;

    /// A shortcut for calling [DisplaySimple::write_framebuffer] followed by [Displayable::update_display].
    async fn display_framebuffer(&mut self, spi: &mut SPI, buf: &dyn BufferView<BITS, FRAMES>) -> Result<(), ERROR>;
}

/// Displays that support a partial update, where a "diff" framebuffer is diffed against a base
/// framebuffer, and only the changed pixels from the diff are actually updated.
pub trait DisplayPartial<const BITS: usize, const FRAMES: usize, SPI: SpiDevice, ERROR>: Displayable<SPI, ERROR> {
    /// Writes the buffer to the base framebuffer that the main framebuffer layer (written with
    /// [DisplaySimple::write_framebuffer]) will be diffed against.
    /// Only pixels that differ will be updated.
    /// 
    /// For standard use, you probably only need to call this once before the first partial display,
    /// as the main framebuffer becomes the diff base after a call to [Displayable::update_display].
    async fn write_base_framebuffer(&mut self, spi: &mut SPI, buf: &dyn BufferView<BITS, FRAMES>) -> Result<(), ERROR>;
}