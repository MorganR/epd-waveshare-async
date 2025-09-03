#![no_std]

use core::convert::Infallible;
use core::marker::PhantomData;

use defmt::error;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice as EmbassySpiDevice;
use embassy_embedded_hal::shared_bus::SpiDeviceError;
use embassy_rp::gpio::{Input, Level, Output, Pin, Pull};
use embassy_rp::spi;
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_time::Delay;
use epd_waveshare_async::{BusyHw, DcHw, DelayHw, Error as EpdError, ErrorHw, ResetHw, SpiHw};
use thiserror::Error as ThisError;
use {defmt_rtt as _, panic_probe as _};

/// Defines the hardware to use for connecting to the display.
pub struct DisplayHw<'a, SPI> {
    dc: Output<'a>,
    reset: Output<'a>,
    busy: Input<'a>,
    delay: Delay,
    _spi_type: PhantomData<SPI>,
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
            _spi_type: PhantomData,
        }
    }
}

pub type RawSpiError = SpiDeviceError<spi::Error, Infallible>;

impl<'a, SPI> ErrorHw for DisplayHw<'a, SPI> {
    type Error = Error;
}

impl<'a, SPI> DcHw for DisplayHw<'a, SPI> {
    type Dc = Output<'a>;

    fn dc(&mut self) -> &mut Self::Dc {
        &mut self.dc
    }
}

impl<'a, SPI> ResetHw for DisplayHw<'a, SPI> {
    type Reset = Output<'a>;

    fn reset(&mut self) -> &mut Self::Reset {
        &mut self.reset
    }
}

impl<'a, SPI> BusyHw for DisplayHw<'a, SPI> {
    type Busy = Input<'a>;

    fn busy(&mut self) -> &mut Self::Busy {
        &mut self.busy
    }
}

impl<'a, SPI> DelayHw for DisplayHw<'a, SPI> {
    type Delay = embassy_time::Delay;

    fn delay(&mut self) -> &mut Self::Delay {
        &mut self.delay
    }
}

impl<'a, SPI: spi::Instance + 'a> SpiHw for DisplayHw<'a, SPI> {
    type Spi = EmbassySpiDevice<'a, NoopRawMutex, spi::Spi<'a, SPI, spi::Async>, Output<'a>>;
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
