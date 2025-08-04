//! This example tests the EPD Waveshare 2.9" display driver using a Raspberry Pi Pico board.

#![no_std]
#![no_main]

use defmt::{expect, info};
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::spi::{self, Spi};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Instant, Timer};
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyle};
use epd_waveshare_async::epd2in9::{self};
use epd_waveshare_async::{
    epd2in9_v2::{Epd2In9V2, RefreshMode},
    *,
};
use rp_samples::*;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let resources = split_resources!(p);
    let mut config = spi::Config::default();
    config.frequency = epd2in9::RECOMMENDED_SPI_HZ;
    // embassy-rp uses the synchronous phase and polarity enums, so we have to map these.
    config.phase = match epd2in9::RECOMMENDED_SPI_PHASE {
        embedded_hal_async::spi::Phase::CaptureOnFirstTransition => {
            embassy_rp::spi::Phase::CaptureOnFirstTransition
        }
        embedded_hal_async::spi::Phase::CaptureOnSecondTransition => {
            embassy_rp::spi::Phase::CaptureOnSecondTransition
        }
    };
    config.polarity = match epd2in9::RECOMMENDED_SPI_POLARITY {
        embedded_hal_async::spi::Polarity::IdleHigh => embassy_rp::spi::Polarity::IdleHigh,
        embedded_hal_async::spi::Polarity::IdleLow => embassy_rp::spi::Polarity::IdleLow,
    };

    let raw_spi: Mutex<NoopRawMutex, _> = Mutex::new(Spi::new_txonly(
        resources.spi_hw.spi,
        resources.spi_hw.clk,
        resources.spi_hw.tx,
        resources.spi_hw.dma_tx,
        config,
    ));
    // CS is active low.
    let cs_pin = Output::new(resources.spi_hw.cs, Level::High);
    let mut spi = SpiDevice::new(&raw_spi, cs_pin);
    let epd = Epd2In9V2::new(DisplayHw::new(resources.epd_hw));

    info!("Initializing EPD");
    let mut epd = expect!(
        epd.init(&mut spi, RefreshMode::Full).await,
        "Failed to initialize EPD"
    );

    let mut buffer = epd2in9_v2::new_binary_buffer();
    buffer
        .fill_solid(&buffer.bounding_box(), BinaryColor::On)
        .unwrap();
    info!("Displaying white buffer");
    // TODO: try properly setting this after low ram, but before turning on the display
    // epd.send(&mut spi, Command::WriteHighRam, buffer.data()).await.unwrap();
    // I suspect I need to set both "low" and "high" buffers before doing a partial update.
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to display buffer"
    );
    Timer::after_secs(4).await;

    // Clear the base image for later partial refresh.
    epd.write_base_framebuffer(&mut spi, &buffer).await.unwrap();

    info!("Displaying text");
    let mut style = TextStyle::default();
    style.alignment = Alignment::Left;
    style.baseline = Baseline::Top;
    let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::Off);
    let text = Text::with_text_style("Hello, EPD!", Point::new(10, 10), character_style, style);
    text.draw(&mut buffer).unwrap();
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to display text buffer"
    );
    Timer::after_secs(4).await;

    info!("Changing to partial refresh mode");
    epd = expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Partial).await,
        "Failed to set refresh mode"
    );

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
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to display check buffer"
    );
    Timer::after_secs(4).await;

    // This works differently from V1: it displays the same image in RefreshMode::Full,
    // or toggles back to the "base" image in RefreshMode::Partial.
    info!("Re-displaying as a test");
    expect!(epd.update_display(&mut spi).await, "Failed to re-display");
    Timer::after_secs(2).await;

    // In partial, this toggles back to the check buffer.
    info!("Re-displaying as a test");
    expect!(epd.update_display(&mut spi).await, "Failed to re-display");
    Timer::after_secs(2).await;

    info!("Sleeping EPD");
    let epd = expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    Timer::after_secs(2).await;

    info!("Waking EPD");
    let mut epd = expect!(epd.wake(&mut spi).await, "Failed to wake EPD");
    Timer::after_secs(1).await;

    info!("Displaying text");
    buffer.clear(BinaryColor::On).unwrap();
    let mut style = TextStyle::default();
    style.alignment = Alignment::Left;
    style.baseline = Baseline::Top;
    let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::Off);
    let text = Text::with_text_style("I'm awake!", Point::new(10, 30), character_style, style);
    text.draw(&mut buffer).unwrap();
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to display text buffer"
    );
    Timer::after_secs(4).await;

    epd = epd
        .set_refresh_mode(&mut spi, RefreshMode::Full)
        .await
        .unwrap();
    buffer.clear(BinaryColor::On).unwrap();
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to clear display"
    );

    let _epd = expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    info!("Done");
}
