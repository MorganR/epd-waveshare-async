#![no_std]

use core::convert::Infallible;
use core::marker::PhantomData;

use embassy_time::Delay;
use embedded_hal::digital::PinState;
use embedded_hal_bus::spi::{DeviceError, ExclusiveDevice};
use epd_waveshare_async::hw::{BusyHw, DcHw, DelayHw, ErrorHw, ResetHw, SpiHw};
use esp_hal::assign_resources;
use esp_hal::{
    Async,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pin},
    peripherals::{GPIO4, GPIO10, GPIO38},
    spi::master::Spi,
};
use thiserror::Error as ThisError;

assign_resources! {
    pub Resources<'r> {
        display:DisplayResources<'r>{
            spi:SPI2,
            mosi:GPIO9,
            clk:GPIO7,
            cs:GPIO44
        },
        epd: EpdResources<'r>{
            busy:GPIO4,
            rst:GPIO38,
            dc:GPIO10
        },
        rtos: RtosResources<'r>{
            timg:TIMG0,
            swint:SW_INTERRUPT
        }
    }
}

/// Defines the hardware to use for connecting to the display.
pub struct DisplayHw<'a, SPI> {
    dc: Output<'a>,
    reset: Output<'a>,
    busy: Input<'a>,
    busy_when: PinState,
    delay: Delay,
    _spi_type: PhantomData<SPI>,
}

impl<'a, SPI> DisplayHw<'a, SPI> {
    pub fn new<DC: Pin, RESET: Pin, BUSY: Pin>(
        dc: GPIO10<'a>,
        reset: GPIO38<'a>,
        busy: GPIO4<'a>,
        busy_when: PinState,
    ) -> Self {
        let dc = Output::new(dc, Level::High, OutputConfig::default());
        let reset = Output::new(reset, Level::High, OutputConfig::default());
        let busy = Input::new(busy, InputConfig::default());

        Self {
            dc,
            reset,
            busy,
            busy_when,
            delay: Delay,
            _spi_type: PhantomData,
        }
    }
}

pub type RawSpiError = esp_hal::spi::Error;

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

    fn busy_when(&self) -> embedded_hal::digital::PinState {
        self.busy_when
    }
}

impl<'a, SPI> DelayHw for DisplayHw<'a, SPI> {
    type Delay = embassy_time::Delay;

    fn delay(&mut self) -> &mut Self::Delay {
        &mut self.delay
    }
}

impl<'a, SPI> SpiHw for DisplayHw<'a, SPI> {
    type Spi = ExclusiveDevice<Spi<'a, Async>, Output<'a>, Delay>;
}

#[derive(Debug, ThisError)]
pub enum Error {
    #[error("SPI error: {0:?}")]
    SpiError(RawSpiError),
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

impl From<DeviceError<RawSpiError, Infallible>> for Error {
    fn from(e: DeviceError<RawSpiError, Infallible>) -> Self {
        match e {
            DeviceError::Spi(e) => Error::SpiError(e),
            DeviceError::Cs(e) => match e {},
        }
    }
}
