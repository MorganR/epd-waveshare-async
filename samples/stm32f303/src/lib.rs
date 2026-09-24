#![no_std]

use core::convert::Infallible;

use defmt_rtt as _; // global logger
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice as EmbassySpiDevice;
use embassy_embedded_hal::shared_bus::SpiDeviceError;
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::{Level, Output, Pin, Speed};
use embassy_stm32::mode::Async;
use embassy_stm32::spi;
use embassy_stm32::Peri;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_time::Delay;
use embedded_hal::digital::PinState;
use epd_waveshare_async::hw::{BusyHw, DcHw, DelayHw, ErrorHw, ResetHw, SpiHw};
use panic_probe as _;
use thiserror::Error as ThisError;

/// Defines the hardware to use for connecting to the display.
pub struct DisplayHw<'a> {
    dc: Output<'a>,
    reset: Output<'a>,
    busy: ExtiInput<'a, Async>,
    busy_when: PinState,
    delay: Delay,
}

impl<'a> DisplayHw<'a> {
    pub fn new(
        dc: Peri<'a, impl Pin>,
        reset: Peri<'a, impl Pin>,
        busy: ExtiInput<'a, Async>,
        busy_when: PinState,
    ) -> Self {
        Self {
            dc: Output::new(dc, Level::High, Speed::Low),
            reset: Output::new(reset, Level::High, Speed::Low),
            busy,
            busy_when,
            delay: Delay,
        }
    }
}

pub type RawSpiError = SpiDeviceError<spi::Error, Infallible>;

impl<'a> ErrorHw for DisplayHw<'a> {
    type Error = Error;
}

impl<'a> DcHw for DisplayHw<'a> {
    type Dc = Output<'a>;

    fn dc(&mut self) -> &mut Self::Dc {
        &mut self.dc
    }
}

impl<'a> ResetHw for DisplayHw<'a> {
    type Reset = Output<'a>;

    fn reset(&mut self) -> &mut Self::Reset {
        &mut self.reset
    }
}

impl<'a> BusyHw for DisplayHw<'a> {
    type Busy = ExtiInput<'a, Async>;

    fn busy(&mut self) -> &mut Self::Busy {
        &mut self.busy
    }

    fn busy_when(&self) -> PinState {
        self.busy_when
    }
}

impl<'a> DelayHw for DisplayHw<'a> {
    type Delay = Delay;

    fn delay(&mut self) -> &mut Self::Delay {
        &mut self.delay
    }
}

impl<'a> SpiHw for DisplayHw<'a> {
    type Spi =
        EmbassySpiDevice<'a, NoopRawMutex, spi::Spi<'a, Async, spi::mode::Master>, Output<'a>>;
}

#[derive(Debug, ThisError, defmt::Format)]
pub enum Error {
    #[error("SPI error: {0:?}")]
    SpiError(RawSpiError),
}

impl From<Infallible> for Error {
    fn from(e: Infallible) -> Self {
        match e {}
    }
}

impl From<RawSpiError> for Error {
    fn from(e: RawSpiError) -> Self {
        Error::SpiError(e)
    }
}
