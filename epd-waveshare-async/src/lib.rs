#![no_std]

use core::error::Error as CoreError;

use embedded_graphics::{prelude::Point, primitives::Rectangle};
use embedded_hal::digital::{ErrorType as PinErrorType, InputPin, OutputPin};
use embedded_hal_async::{
    delay::DelayNs,
    digital::Wait,
    spi::{ErrorType as SpiErrorType, SpiBus},
};

use crate::epd2in9::RefreshMode;

pub mod buffer;
pub mod epd2in9;

mod log;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Error {
    InvalidArgument,
}

#[allow(async_fn_in_trait)]
pub trait Epd<HW>
where
    HW: EpdHw,
{
    type RefreshMode;
    type Command;
    type Buffer;

    /// Initialise the display. This must be called before any other operations.
    async fn init(&mut self, spi: &mut HW::Spi, mode: RefreshMode) -> Result<(), HW::Error>;

    /// Creates a buffer for use with this display.
    fn buffer(&self) -> Self::Buffer;

    /// Sets the refresh mode for the display.
    async fn set_refresh_mode(
        &mut self,
        spi: &mut HW::Spi,
        mode: Self::RefreshMode,
    ) -> Result<(), HW::Error>;

    /// Hardware reset the display. The display must be reinitialised after calling this.
    async fn reset(&mut self) -> Result<(), HW::Error>;

    /// Puts the display to sleep.
    async fn sleep(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error>;

    /// Wakes and re-initialises the display if it's asleep.
    async fn wake(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error>;

    /// Writes the buffers data to the display and displays it.
    async fn display_buffer(
        &mut self,
        spi: &mut HW::Spi,
        buffer: &Self::Buffer,
    ) -> Result<(), HW::Error>;

    /// Sets the window to write to during a call to [write_image]. This can enable partial writes
    /// to a subsection of the display.
    async fn set_window(&mut self, spi: &mut HW::Spi, shape: Rectangle) -> Result<(), HW::Error>;

    /// Sets the cursor position for where the next byte of image data will be written.
    async fn set_cursor(
        &mut self,
        spi: &mut HW::Spi,
        position: Point,
    ) -> Result<(), <HW as EpdHw>::Error>;

    /// Writes raw image data, starting at the current cursor position and auto-incrementing x then y within the current window.
    async fn write_image(&mut self, spi: &mut HW::Spi, image: &[u8]) -> Result<(), HW::Error>;

    /// Updates (refreshes) the display to match the latest data that has been written to RAM.
    async fn update_display(&mut self, spi: &mut HW::Spi) -> Result<(), HW::Error>;

    /// Send the following command and data to the display. Waits until the display is no longer busy before sending.
    async fn send(
        &mut self,
        spi: &mut HW::Spi,
        command: Self::Command,
        data: &[u8],
    ) -> Result<(), HW::Error>;

    /// Waits for the current operation to complete if the display is busy.
    /// Note that this will wait forever if the display is asleep.
    async fn wait_if_busy(&mut self) -> Result<(), HW::Error>;
}

/// Provides access to the hardware needed to control an EPD.
pub trait EpdHw {
    type Spi: SpiBus;
    type Cs: OutputPin;
    type Dc: OutputPin;
    type Reset: OutputPin;
    type Busy: InputPin + Wait;
    type Delay: DelayNs;
    type Error: CoreError
        + From<<Self::Spi as SpiErrorType>::Error>
        + From<<Self::Cs as PinErrorType>::Error>
        + From<<Self::Dc as PinErrorType>::Error>
        + From<<Self::Reset as PinErrorType>::Error>
        + From<<Self::Busy as PinErrorType>::Error>
        + From<Error>;

    fn cs(&mut self) -> &mut Self::Cs;
    fn dc(&mut self) -> &mut Self::Dc;
    fn reset(&mut self) -> &mut Self::Reset;
    fn busy(&mut self) -> &mut Self::Busy;
    fn delay(&mut self) -> &mut Self::Delay;
}
