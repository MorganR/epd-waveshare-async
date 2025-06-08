//! This example tests the EPD Waveshare 2.9" display driver using a Raspberry Pi Pico board.

#![no_std]
#![no_main]

use core::convert::Infallible;

use defmt::{error, expect, info};
use embassy_executor::Spawner;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::spi::{self, Spi};
use embassy_rp::peripherals;
use embassy_time::{Delay, Timer};
use epd_waveshare_async::{
    epd2in9::{self, Epd2in9},
    Epd, EpdHw,
};
use thiserror::Error as ThisError;
use {defmt_rtt as _, panic_probe as _};

assign_resources::assign_resources! {
    spi_hw: SpiP {
        spi: SPI0,
        clk: PIN_2,
        tx: PIN_3,
        rx: PIN_4,
        dma_tx: DMA_CH0,
        dma_rx: DMA_CH1,
    },
    epd_hw: DisplayP {
        cs: PIN_5,
        reset: PIN_7,
        dc: PIN_6,
        busy: PIN_8,
    },
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let resources = split_resources!(p);
    let mut config = spi::Config::default();
    // TOOD: Put these settings in the driver.
    config.frequency = 4_000_000;
    config.phase = spi::Phase::CaptureOnFirstTransition;
    config.polarity = spi::Polarity::IdleLow;

    let mut spi = Spi::new(
        resources.spi_hw.spi,
        resources.spi_hw.clk,
        resources.spi_hw.tx,
        resources.spi_hw.rx,
        resources.spi_hw.dma_tx,
        resources.spi_hw.dma_rx,
        config,
    );
    let mut epd = Epd2in9::new(DisplayHw::new(resources.epd_hw));

    info!("Initializing EPD");
    expect!(
        epd.init(&mut spi, &epd2in9::LUT_FULL_UPDATE).await,
        "Failed to initialize EPD"
    );
    Timer::after_secs(5).await;

    info!("Clearing EPD");
    expect!(epd.clear(&mut spi).await, "Failed to clear EPD");
    Timer::after_secs(5).await;

    info!("Sleeping EPD");
    expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    info!("Done");
}

struct DisplayHw<'a> {
    cs: Output<'a>,
    dc: Output<'a>,
    reset: Output<'a>,
    busy: Input<'a>,
    delay: Delay,
}

impl<'a> DisplayHw<'a> {
    fn new(p: DisplayP) -> Self {
        let cs = Output::new(p.cs, Level::High);
        let dc = Output::new(p.dc, Level::High);
        let reset = Output::new(p.reset, Level::High);
        let busy = Input::new(p.busy, Pull::Up);

        Self {
            cs,
            dc,
            reset,
            busy,
            delay: Delay,
        }
    }
}

impl<'a> EpdHw for DisplayHw<'a> {
    type Spi = Spi<'a, peripherals::SPI0, spi::Async>;

    type Cs = Output<'a>;

    type Dc = Output<'a>;

    type Reset = Output<'a>;

    type Busy = Input<'a>;

    type Delay = embassy_time::Delay;

    type Error = Error;

    fn cs(&mut self) -> &mut Self::Cs {
        &mut self.cs
    }

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
enum Error {
    #[error("Spi error: {0:?}")]
    SpiError(spi::Error),
    #[error("EPD error: {0:?}")]
    EpdError(epd_waveshare_async::Error),
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

impl From<spi::Error> for Error {
    fn from(e: spi::Error) -> Self {
        Error::SpiError(e)
    }
}

impl From<epd_waveshare_async::Error> for Error {
    fn from(e: epd_waveshare_async::Error) -> Self {
        Error::EpdError(e)
    }
}
