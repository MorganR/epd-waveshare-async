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
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyle};
use epd_waveshare_async::{
    epd2in9::{Epd2in9, RefreshMode},
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
        dma_tx: DMA_CH1,
        dma_rx: DMA_CH2,
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
        epd.init(&mut spi, RefreshMode::Full).await,
        "Failed to initialize EPD"
    );

    let mut buffer = epd.buffer();
    buffer.fill_solid(&buffer.bounding_box(), BinaryColor::On).unwrap();
    info!("Displaying white buffer");
    expect!(
        epd.display_buffer(&mut spi, &buffer).await,
        "Failed to display buffer"
    );
    Timer::after_secs(5).await;

    info!("Changing to partial refresh mode");
    expect!(epd.set_refresh_mode(&mut spi, RefreshMode::Partial)
        .await, 
        "Failed to set refresh mode");

    info!("Displaying text");
    let mut style = TextStyle::default();
        style.alignment = Alignment::Left;
        style.baseline = Baseline::Top;
        let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::Off);
        let text = Text::with_text_style("Hello, EPD!", Point::new(10, 10), character_style, style);
    text.draw(&mut buffer).unwrap();
    expect!(epd.display_buffer(&mut spi, &buffer).await, "Failed to display text buffer");
    Timer::after_secs(5).await;

    // TODO: This isn't displaying as expected.
    info!("Displaying check buffer");
    // Clear first.
    buffer.fill_solid(&buffer.bounding_box(), BinaryColor::On).unwrap();
    let mut top_left = Point::new(0, 0);
    let mut box_size = buffer.bounding_box().size.width;
    let mut color = BinaryColor::Off;
    while box_size > 0 {
        buffer.fill_solid(&Rectangle::new(top_left, Size::new(box_size, box_size)), color).unwrap();
        color = match color {
            BinaryColor::On => BinaryColor::Off,
            BinaryColor::Off => BinaryColor::On,
        };
        if (top_left.x + box_size as i32) >= buffer.bounding_box().size.width as i32 {
            top_left.x = 0;
            top_left.y += box_size as i32;
        } else {
            top_left.x += box_size as i32;
        }
        box_size /= 2;
    }
    expect!(
        epd.display_buffer(&mut spi, &buffer).await,
        "Failed to display buffer"
    );
    Timer::after_secs(5).await;

    info!("Sleeping EPD");
    expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    Timer::after_secs(2).await;

    info!("Waking EPD");
    expect!(epd.wake(&mut spi).await, "Failed to wake EPD");
    Timer::after_secs(2).await;

    info!("Bypassing to old RAM");
    expect!(epd.select_ram(&mut spi, 1).await, "Failed to bypass RAM");
    expect!(epd.update_display(&mut spi).await, "Failed to update display");
    Timer::after_secs(5).await;

    info!("Back to new RAM");
    expect!(epd.select_ram(&mut spi, 0).await, "Failed to undo RAM bypass");
    expect!(epd.update_display(&mut spi).await, "Failed to update display");
    Timer::after_secs(5).await;

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
    #[error("SPI error: {0:?}")]
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
