//! This example tests the EPD Waveshare 2.9" display driver using a Raspberry Pi Pico board.

#![no_std]
#![no_main]

use core::convert::Infallible;

use defmt::{error, expect, info};
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_embedded_hal::shared_bus::SpiDeviceError;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals;
use embassy_rp::spi::{self, Spi};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Delay, Instant, Timer};
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
        cs: PIN_5,
    },
    epd_hw: DisplayP {
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

    let raw_spi: Mutex<NoopRawMutex, _> = Mutex::new(Spi::new(
        resources.spi_hw.spi,
        resources.spi_hw.clk,
        resources.spi_hw.tx,
        resources.spi_hw.rx,
        resources.spi_hw.dma_tx,
        resources.spi_hw.dma_rx,
        config,
    ));
    // CS is active low.
    let cs_pin = Output::new(resources.spi_hw.cs, Level::High);
    let mut spi = SpiDevice::new(&raw_spi, cs_pin);
    let mut epd = Epd2in9::new(DisplayHw::new(resources.epd_hw));

    info!("Initializing EPD");
    expect!(
        epd.init(&mut spi, RefreshMode::Full).await,
        "Failed to initialize EPD"
    );

    let mut buffer = epd.new_buffer();
    buffer
        .fill_solid(&buffer.bounding_box(), BinaryColor::On)
        .unwrap();
    info!("Displaying white buffer");
    expect!(
        epd.display_buffer(&mut spi, &buffer).await,
        "Failed to display buffer"
    );
    Timer::after_secs(4).await;

    info!("Changing to partial refresh mode");
    expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Partial).await,
        "Failed to set refresh mode"
    );

    info!("Displaying text");
    let mut style = TextStyle::default();
    style.alignment = Alignment::Left;
    style.baseline = Baseline::Top;
    let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::Off);
    let text = Text::with_text_style("Hello, EPD!", Point::new(10, 10), character_style, style);
    text.draw(&mut buffer).unwrap();
    expect!(
        epd.display_buffer(&mut spi, &buffer).await,
        "Failed to display text buffer"
    );
    Timer::after_secs(4).await;

    info!("Displaying check buffer");
    let before_buffer_draw = Instant::now();
    // Clear first.
    buffer
        .fill_solid(&buffer.bounding_box(), BinaryColor::On)
        .unwrap();
    let mut top_left = Point::new(0, 0);
    let buffer_width = buffer.bounding_box().size.width;
    let mut box_size = buffer_width;
    let mut color = BinaryColor::Off;
    while box_size > 0 {
        for _ in 0..(buffer_width / box_size) {
            buffer
                .fill_solid(
                    &Rectangle::new(top_left, Size::new(box_size, box_size)),
                    color,
                )
                .unwrap();
            color = color.invert();
            top_left.x += box_size as i32;
        }
        top_left.x = 0;
        top_left.y += box_size as i32;
        color = color.invert();
        box_size /= 2;
    }
    let after_buffer_draw = Instant::now();
    info!(
        "Check buffer drawn in {} ms",
        (after_buffer_draw - before_buffer_draw).as_millis()
    );
    expect!(
        epd.display_buffer(&mut spi, &buffer).await,
        "Failed to display text buffer"
    );
    Timer::after_secs(4).await;

    info!("Sleeping EPD");
    expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    Timer::after_secs(2).await;

    info!("Waking EPD");
    expect!(epd.wake(&mut spi).await, "Failed to wake EPD");
    Timer::after_secs(1).await;

    expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Full).await,
        "Failed to set refresh mode"
    );
    info!("Setting white border");
    expect!(
        epd.set_border(&mut spi, BinaryColor::On).await,
        "Failed to set border color"
    );
    expect!(
        epd.update_display(&mut spi).await,
        "Failed to update display"
    );
    Timer::after_secs(3).await;

    info!("Setting black border");
    expect!(
        epd.set_border(&mut spi, BinaryColor::Off).await,
        "Failed to set border color"
    );
    expect!(
        epd.update_display(&mut spi).await,
        "Failed to update display"
    );
    Timer::after_secs(3).await;

    expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    info!("Done");
}

struct DisplayHw<'a> {
    dc: Output<'a>,
    reset: Output<'a>,
    busy: Input<'a>,
    delay: Delay,
}

impl DisplayHw<'_> {
    fn new(p: DisplayP) -> Self {
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

type RawSpiError = SpiDeviceError<spi::Error, Infallible>;

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
enum Error {
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
