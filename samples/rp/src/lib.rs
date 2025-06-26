#![no_std]

use core::convert::Infallible;

use defmt::error;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_embedded_hal::shared_bus::SpiDeviceError;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals;
use embassy_rp::spi::{self, Spi};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_time::Delay;
use epd_waveshare_async::EpdHw;
use thiserror::Error as ThisError;
use {defmt_rtt as _, panic_probe as _};

// Define the resources needed to communicate with the display.
assign_resources::assign_resources! {
    spi_hw: SpiP {
        spi: SPI0,
        clk: PIN_2,
        tx: PIN_3,
        rx: PIN_4,
        dma_tx: DMA_CH1,
        dma_rx: DMA_CH2,
        cs: PIN_5,
    },
    epd_hw: DisplayP {
        reset: PIN_7,
        dc: PIN_6,
        busy: PIN_8,
    },
}

/// Defines the hardware to use for connecting to the display.
pub struct DisplayHw<'a> {
    dc: Output<'a>,
    reset: Output<'a>,
    busy: Input<'a>,
    delay: Delay,
}

impl DisplayHw<'_> {
    pub fn new(p: DisplayP) -> Self {
        let dc = Output::new(p.dc, Level::High);
        let reset = Output::new(p.reset, Level::High);
        let busy = Input::new(p.busy, Pull::Up);

        Self {
            dc,
            reset,
            busy,
            delay: Delay,
        }
    }
}

pub type RawSpiError = SpiDeviceError<spi::Error, Infallible>;

type EpdSpiDevice<'a> =
    SpiDevice<'a, NoopRawMutex, Spi<'a, peripherals::SPI0, spi::Async>, Output<'a>>;

impl<'a> EpdHw for DisplayHw<'a> {
    type Spi = EpdSpiDevice<'a>;

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
