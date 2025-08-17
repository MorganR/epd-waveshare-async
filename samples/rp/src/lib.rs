#![no_std]

use core::convert::Infallible;
use core::marker::PhantomData;

use defmt::error;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_embedded_hal::shared_bus::SpiDeviceError;
use embassy_rp::gpio::{Input, Level, Output, Pin, Pull};
use embassy_rp::spi::{self, Spi};
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_time::Delay;
use epd_waveshare_async::{EPDPowerHw, EpdHw, Error as EpdError};
use thiserror::Error as ThisError;
use {defmt_rtt as _, panic_probe as _};

/// Defines the hardware to use for connecting to the display.
pub struct DisplayHw<'a, SPI: spi::Instance> {
    dc: Output<'a>,
    reset: Output<'a>,
    busy: Input<'a>,
    delay: Delay,
    _spi: PhantomData<SPI>,
}

impl<'a, SPI: spi::Instance> DisplayHw<'a, SPI> {
    pub fn new<DC: Pin, RESET: Pin, BUSY: Pin>(
        dc: Peri<'a, DC>,
        reset: Peri<'a, RESET>,
        busy: Peri<'a, BUSY>,
    ) -> Self {
        let dc = Output::new(dc, Level::High);
        let reset = Output::new(reset, Level::High);
        let busy = Input::new(busy, Pull::Up);

        Self {
            dc,
            reset,
            busy,
            delay: Delay,
            _spi: PhantomData {},
        }
    }
}

pub struct DisplayPowerHw<'a> {
    power: Output<'a>,
}

impl<'a> DisplayPowerHw<'a> {
    pub fn new<POWER: Pin>(power: Peri<'a, POWER>) -> Self {
        let power = Output::new(power, Level::Low);

        Self { power }
    }
}
pub type RawSpiError = SpiDeviceError<spi::Error, Infallible>;

type EpdSpiDevice<'a, SPI> = SpiDevice<'a, NoopRawMutex, Spi<'a, SPI, spi::Async>, Output<'a>>;

impl<'a, SPI: spi::Instance + 'a> EpdHw for DisplayHw<'a, SPI> {
    type Spi = EpdSpiDevice<'a, SPI>;

    type Dc = Output<'a>;

    type Reset = Output<'a>;

    type Busy = Input<'a>;

    type Delay = embassy_time::Delay;

    type Error = Error;

    fn dc(&mut self) -> &mut Self::Dc {
        &mut self.dc
    }

    fn reset(&mut self) -> &mut Self::Reset {
        &mut self.reset
    }

    fn busy(&mut self) -> &mut Self::Busy {
        &mut self.busy
    }

    fn delay(&mut self) -> &mut Self::Delay {
        &mut self.delay
    }
}

impl<'a> EPDPowerHw for DisplayPowerHw<'a> {
    type Power = Output<'a>;
    type Error = Error;

    fn power(&mut self) -> &mut Self::Power {
        &mut self.power
    }
}

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("SPI error: {0:?}")]
    SpiError(RawSpiError),
    #[error("Display error: {0:?}")]
    DisplayError(EpdError),
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

impl From<RawSpiError> for Error {
    fn from(e: RawSpiError) -> Self {
        Error::SpiError(e)
    }
}

impl From<EpdError> for Error {
    fn from(e: EpdError) -> Self {
        Error::DisplayError(e)
    }
}
