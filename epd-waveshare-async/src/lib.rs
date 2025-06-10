#![no_std]

use core::error::Error as CoreError;

use embedded_graphics::{prelude::Point, primitives::Rectangle};
use embedded_hal::digital::{ErrorType as PinErrorType, InputPin, OutputPin};
use embedded_hal_async::{
    delay::DelayNs,
    digital::Wait,
    spi::{ErrorType as SpiErrorType, SpiDevice},
};

use crate::epd2in9::RefreshMode;

pub mod buffer;
pub mod epd2in9;

mod log;

#[allow(async_fn_in_trait)]
pub trait Epd<HW>
where
    HW: EpdHw,
{
    type RefreshMode;
    type Command;
    type Buffer;

    /// Creates a buffer for use with this display.
    fn new_buffer(&self) -> Self::Buffer;

    fn width(&self) -> u32;

    fn height(&self) -> u32;

    /// Initialise the display. This must be called before any other operations.
    async fn init(&mut self, spi: &mut HW::Spi, mode: RefreshMode) -> Result<(), HW::Error>;

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

    /// Updates (refreshes) the display based on the RAM. Note that this can be stateful. For
    /// example, on the Epd2in9 display, there are two RAM buffers. Calling this function swaps
    /// the active buffer. Consider this scenario:
    ///
    /// 1. [write_image] is used to turn the RAM all white.
    /// 2. [update_display] is called, which refreshes the display to be all white.
    /// 3. [write_image] is used to turn the RAM all black.
    /// 4. [update_display] is called, which refreshes the display to be all black.
    /// 5. [update_display] is called again, which refreshes the display to be all white again.
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
