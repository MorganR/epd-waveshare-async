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

/// Indicates display errors due to incorrect states.
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

pub trait Reset<ERROR> {
    /// Hardware resets the display.
    async fn reset(&mut self) -> Result<(), ERROR>;
}

/// Displays that can sleep to save power.
pub trait Sleep<SPI: SpiDevice, ERROR> {
    /// Puts the display to sleep.
    async fn sleep(&mut self, spi: &mut SPI) -> Result<(), ERROR>;

    /// Wakes and re-initialises the display (if necessary) if it's asleep.
    async fn wake(&mut self, spi: &mut SPI) -> Result<(), ERROR>;
}

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

pub trait DisplaySimple<const BITS: usize, const FRAMES: usize, SPI: SpiDevice, ERROR>: Displayable<SPI, ERROR> {
    async fn write_framebuffer(&mut self, spi: &mut SPI, buf: &dyn BufferView<BITS, FRAMES>) -> Result<(), ERROR>;

    async fn display_framebuffer(&mut self, spi: &mut SPI, buf: &dyn BufferView<BITS, FRAMES>) -> Result<(), ERROR>;
}

pub trait DisplayPartial<const BITS: usize, const FRAMES: usize, SPI: SpiDevice, ERROR>: Displayable<SPI, ERROR> {
    /// Writes the buffer to the base framebuffer that the "diff" layer will be diffed against.
    /// Only pixels that differ will be updated.
    /// 
    /// For standard use, you probably only need to call this once before the first partial display.
    async fn write_base_framebuffer(&mut self, spi: &mut SPI, buf: &dyn BufferView<BITS, FRAMES>) -> Result<(), ERROR>;
    /// Writes the buffer to the diff layer, which will be diffed against the base. Only pixels that
    /// differ will be updated.
    /// 
    /// On calls to [Displayable::update_display], the diff and base framebuffers should
    /// automatically be swapped. In most cases, this means that [DisplayPartial::write_base_framebuffer]
    /// only needs to be called once to support repeated partial refreshes.
    async fn write_diff_framebuffer(&mut self, spi: &mut SPI, buf: &dyn BufferView<BITS, FRAMES>) -> Result<(), ERROR>;
}