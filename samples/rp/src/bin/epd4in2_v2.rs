//! This example tests the EPD Waveshare 4.2" v2 display driver using a Raspberry Pi Pico board.

#![no_std]
#![no_main]

use defmt::{expect, info};
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals;
use embassy_rp::spi::{self, Spi};
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::{BinaryColor, Gray2};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyle};
use epd_waveshare_async::epd4in2_v2::{self, Epd4In2V2, RefreshMode};
use epd_waveshare_async::{DisplayPartial, DisplaySimple, Displayable, Sleep, Wake};
use rp_samples::*;
use {defmt_rtt as _, panic_probe as _};

// Define the resources needed to communicate with the display.
assign_resources::assign_resources! {
    spi_hw: SpiP {
        spi: SPI1,
        clk: PIN_10,
        tx: PIN_11,
        dma_tx: DMA_CH1,
        cs: PIN_9,
    },
    epd_hw: DisplayP {
        reset: PIN_12,
        dc: PIN_8,
        busy: PIN_13,
    },
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let resources = split_resources!(p);
    let mut config = spi::Config::default();
    config.frequency = epd4in2_v2::RECOMMENDED_SPI_HZ;

    // embassy-rp uses the synchronous phase and polarity enums, so we have to map these.
    config.phase = match epd4in2_v2::RECOMMENDED_SPI_PHASE {
        embedded_hal_async::spi::Phase::CaptureOnFirstTransition => {
            embassy_rp::spi::Phase::CaptureOnFirstTransition
        }
        embedded_hal_async::spi::Phase::CaptureOnSecondTransition => {
            embassy_rp::spi::Phase::CaptureOnSecondTransition
        }
    };

    config.polarity = match epd4in2_v2::RECOMMENDED_SPI_POLARITY {
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
    let epd = Epd4In2V2::new(DisplayHw::new(
        resources.epd_hw.dc,
        resources.epd_hw.reset,
        resources.epd_hw.busy,
        epd4in2_v2::DEFAULT_BUSY_WHEN,
    ));

    info!("Initializing EPD");
    let mut epd = expect!(
        epd.init(&mut spi, RefreshMode::Fast).await,
        "Failed to initialize EPD"
    );

    let mut buffer = epd4in2_v2::new_binary_buffer();
    buffer
        .fill_solid(&buffer.bounding_box(), BinaryColor::On)
        .unwrap();
    info!("Displaying white buffer");
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to display buffer"
    );
    Timer::after_secs(4).await;

    info!("Displaying text");
    let mut text_style = TextStyle::default();
    text_style.alignment = Alignment::Left;
    text_style.baseline = Baseline::Top;
    let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::Off);
    let text = Text::with_text_style(
        "Hello, EPD!",
        Point::new(10, 10),
        character_style,
        text_style,
    );
    text.draw(&mut buffer).unwrap();
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to display text buffer"
    );
    Timer::after_secs(4).await;

    info!("Changing to partial refresh mode");
    expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Partial).await,
        "Failed to set refresh mode"
    );

    info!("Displaying black text on white");
    // The partial update diffs the current framebuffer against the base framebuffer, and only
    // drives pixels where they differ. The base must therefore mirror what's on screen, so we
    // write the newly displayed image back to the base after each update.
    buffer.clear(BinaryColor::On).unwrap();
    let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::Off);
    let text = Text::with_text_style(
        "Black text",
        Point::new(10, 30),
        character_style,
        text_style,
    );
    text.draw(&mut buffer).unwrap();
    epd.write_framebuffer(&mut spi, &buffer).await.unwrap();
    epd.update_display(&mut spi).await.unwrap();
    epd.write_base_framebuffer(&mut spi, &buffer).await.unwrap();
    Timer::after_secs(4).await;

    info!("Sleeping EPD");
    let epd = expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    Timer::after_secs(2).await;

    info!("Waking EPD");
    let mut epd = expect!(epd.wake(&mut spi).await, "Failed to wake EPD");
    Timer::after_secs(1).await;

    info!("Changing to fast refresh mode");
    expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Fast).await,
        "Failed to set refresh mode"
    );

    info!("Re-displaying black text");
    // The reset on wake clears the controller's RAM, so the display must be re-rendered. A full/fast
    // refresh also leaves the base framebuffer matching the on-screen image, ready for the partial
    // updates below.
    buffer.clear(BinaryColor::On).unwrap();
    let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::Off);
    let text = Text::with_text_style(
        "Black text",
        Point::new(10, 30),
        character_style,
        text_style,
    );
    text.draw(&mut buffer).unwrap();
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to display black text buffer"
    );
    Timer::after_secs(4).await;

    info!("Changing to partial refresh mode");
    expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Partial).await,
        "Failed to set refresh mode"
    );

    info!("Displaying I'm awake!");
    buffer.clear(BinaryColor::On).unwrap();
    let text = Text::with_text_style(
        "I'm awake!",
        Point::new(10, 30),
        character_style,
        text_style,
    );
    text.draw(&mut buffer).unwrap();
    // The partial update diffs the current framebuffer against the base framebuffer, and only
    // drives pixels where they differ. The base must therefore mirror what's on screen, so we
    // write the newly displayed image back to the base after each update.
    epd.write_framebuffer(&mut spi, &buffer).await.unwrap();
    epd.update_display(&mut spi).await.unwrap();
    epd.write_base_framebuffer(&mut spi, &buffer).await.unwrap();
    Timer::after_secs(4).await;

    info!("Displaying white text on black");
    buffer.clear(BinaryColor::Off).unwrap();
    let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let text = Text::with_text_style(
        "White text",
        Point::new(10, 60),
        character_style,
        text_style,
    );
    text.draw(&mut buffer).unwrap();
    // Set the Border color to black
    epd.set_border_waveform(&mut spi, epd4in2_v2::BorderWaveform::Black)
        .await
        .unwrap();
    epd.write_framebuffer(&mut spi, &buffer).await.unwrap();
    epd.update_display(&mut spi).await.unwrap();
    epd.set_border_waveform(&mut spi, epd4in2_v2::BorderWaveform::Default)
        .await
        .unwrap();
    epd.write_base_framebuffer(&mut spi, &buffer).await.unwrap();
    Timer::after_secs(4).await;

    info!("Display 4-color grayscale");
    let mut gray_buffer = epd4in2_v2::new_gray2_buffer();
    let square_size = Size::new(
        gray_buffer.bounding_box().size.width / 4,
        gray_buffer.bounding_box().size.height,
    );
    let square_step = Size::new(square_size.width, 0);
    let mut start = Point::new(0, 0);
    for luma in 0..4 {
        gray_buffer
            .fill_solid(&Rectangle::new(start, square_size), Gray2::new(luma))
            .unwrap();
        start += square_step;
    }

    expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Gray2).await,
        "Failed to set Gray2 refresh mode"
    );
    expect!(
        epd.display_framebuffer(&mut spi, &gray_buffer).await,
        "Failed to draw Gray2 buffer"
    );
    Timer::after_secs(6).await;

    expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Full).await,
        "Failed to set Full refresh mode"
    );
    buffer.clear(BinaryColor::On).unwrap();
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to clear display"
    );
    Timer::after_secs(1).await;

    let _epd = expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    info!("Done");
}
