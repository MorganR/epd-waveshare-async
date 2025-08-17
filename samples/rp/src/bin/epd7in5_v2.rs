//! This example tests the EPD Waveshare 7.5" display driver using a Raspberry Pi Pico board.

#![no_std]
#![no_main]

use defmt::{debug, expect, info};
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals;
use embassy_rp::spi::{self, Spi};
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyle};
use epd_waveshare_async::epd7in5_v2::{self};
use epd_waveshare_async::epd7in5_v2::{Epd7In5v2, RefreshMode};
use epd_waveshare_async::{
    DisplayPartial, DisplayPartialArea, DisplaySimple, Displayable, PowerOff, PowerOn, Sleep, Wake,
};
use rp_samples::{DisplayHw, DisplayPowerHw};
use {defmt_rtt as _, panic_probe as _};

assign_resources::assign_resources! {
    spi_hw: SpiP {
        spi: SPI1,
        clk: PIN_10,
        tx: PIN_11,
        dma_tx: DMA_CH3,
        cs: PIN_9,
    },
    epd_hw: DisplayP {
        reset: PIN_12,
        dc: PIN_8,
        busy: PIN_13,
    },
    epd_phw: PowerP {
        power: PIN_14,
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let resources = split_resources!(p);
    let mut config = spi::Config::default();
    config.frequency = epd7in5_v2::RECOMMENDED_SPI_HZ;
    // embassy-rp uses the synchronous phase and polarity enums, so we have to map these.
    config.phase = match epd7in5_v2::RECOMMENDED_SPI_PHASE {
        embedded_hal_async::spi::Phase::CaptureOnFirstTransition => {
            spi::Phase::CaptureOnFirstTransition
        }
        embedded_hal_async::spi::Phase::CaptureOnSecondTransition => {
            spi::Phase::CaptureOnSecondTransition
        }
    };
    config.polarity = match epd7in5_v2::RECOMMENDED_SPI_POLARITY {
        embedded_hal_async::spi::Polarity::IdleHigh => spi::Polarity::IdleHigh,
        embedded_hal_async::spi::Polarity::IdleLow => spi::Polarity::IdleLow,
    };

    let raw_spi: Mutex<NoopRawMutex, _> = Mutex::new(Spi::new_txonly(
        resources.spi_hw.spi,
        resources.spi_hw.clk,
        resources.spi_hw.tx,
        resources.spi_hw.dma_tx,
        config,
    ));

    // CS is active low.
    let cs_pin = Output::new(resources.spi_hw.cs, Level::Low);
    let mut spi = SpiDevice::new(&raw_spi, cs_pin);
    let epd = Epd7In5v2::new(
        DisplayHw::new(
            resources.epd_hw.dc,
            resources.epd_hw.reset,
            resources.epd_hw.busy,
        ),
        DisplayPowerHw::new(resources.epd_phw.power),
    );

    info!("Initializing EPD");
    let epd = expect!(epd.power_on().await, "Failed to power on the EPD");
    let mut epd = expect!(
        epd.init(&mut spi, RefreshMode::Full).await,
        "Failed to initialize EPD"
    );
    info!("Initialized EPD");

    let mut buffer = epd7in5_v2::new_binary_buffer();
    buffer
        .fill_solid(&buffer.bounding_box(), BinaryColor::Off)
        .unwrap();

    let rect = Rectangle::new(Point::new(200, 0), Size::new(400, 80));
    buffer.fill_solid(&rect, BinaryColor::On).unwrap();

    let rect = Rectangle::new(Point::new(200, 100), Size::new(400, 80));
    buffer.fill_solid(&rect, BinaryColor::On).unwrap();

    let rect = Rectangle::new(Point::new(200, 200), Size::new(400, 80));
    buffer.fill_solid(&rect, BinaryColor::On).unwrap();

    let rect = Rectangle::new(Point::new(200, 300), Size::new(400, 80));
    buffer.fill_solid(&rect, BinaryColor::On).unwrap();

    info!("Displaying four horizontal blocks");
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to display buffer"
    );
    Timer::after_secs(5).await;

    info!("Changing to partial refresh mode");
    let mut epd = expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Partial).await,
        "Failed to set refresh mode"
    );

    info!("Setting base framebuffer to white to force update of changed pixels");
    buffer.clear(BinaryColor::Off).unwrap();
    epd.write_base_framebuffer(&mut spi, &buffer).await.unwrap();

    info!("Displaying text");
    let hello_str = "Hello, EPD!";
    for t in 1..=hello_str.len() {
        let mut style = TextStyle::default();
        style.alignment = Alignment::Left;
        style.baseline = Baseline::Top;
        let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let text =
            Text::with_text_style(&hello_str[0..t], Point::new(10, 10), character_style, style);
        text.draw(&mut buffer).unwrap();
        expect!(
            epd.display_partial_framebuffer(&mut spi, &buffer, text.bounding_box())
                .await,
            "Failed to display text buffer"
        );
        Timer::after_millis(500).await;
    }
    Timer::after_secs(4).await;

    // Display text with fast refresh mode
    let mut epd = expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Fast).await,
        "Failed to switch to fast refresh m"
    );
    info!("Displaying second text");
    buffer
        .fill_solid(&buffer.bounding_box(), BinaryColor::Off)
        .unwrap();
    let mut style = TextStyle::default();
    style.alignment = Alignment::Center;
    style.baseline = Baseline::Top;
    let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let text = Text::with_text_style(
        "Hello, World!",
        buffer.bounding_box().center(),
        character_style,
        style,
    );
    text.draw(&mut buffer).unwrap();
    debug!(
        "Text bounding box: ({},{}) ({},{})",
        text.bounding_box().top_left.x,
        text.bounding_box().top_left.y,
        text.bounding_box().size.width,
        text.bounding_box().size.height
    );
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to display text buffer"
    );
    Timer::after_secs(4).await;

    // Display clock with partial refresh mode
    let mut epd = expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Partial).await,
        "Failed to set EPD to partial refresh mode"
    );
    buffer
        .fill_solid(&buffer.bounding_box(), BinaryColor::Off)
        .unwrap();
    epd.write_base_framebuffer(&mut spi, &buffer).await.unwrap();
    for time_str in ["12:24:31", "12:24:32", "12:24:33", "12:24:34", "12:24:35"] {
        let mut style = TextStyle::default();
        style.alignment = Alignment::Center;
        style.baseline = Baseline::Top;
        let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let text = Text::with_text_style(time_str, Point::new(200, 300), character_style, style);
        text.draw(&mut buffer).unwrap();
        debug!(
            "Text bounding box: ({},{}) ({},{})",
            text.bounding_box().top_left.x,
            text.bounding_box().top_left.y,
            text.bounding_box().size.width,
            text.bounding_box().size.height
        );
        expect!(
            epd.display_partial_framebuffer(&mut spi, &buffer, text.bounding_box())
                .await,
            "Failed to display text buffer"
        );
        // Clear the text for the next update
        buffer
            .fill_solid(&text.bounding_box(), BinaryColor::Off)
            .unwrap();
        Timer::after_millis(1000).await;
    }

    let epd = expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");

    Timer::after_secs(4).await;
    let epd = expect!(epd.wake(&mut spi).await, "Failed to wake EPD");
    let mut epd = expect!(
        epd.init(&mut spi, RefreshMode::Fast).await,
        "Failed to initialize EPD"
    );

    for i in 0..5 {
        buffer
            .fill_solid(&buffer.bounding_box(), BinaryColor::On)
            .unwrap();
        epd.write_base_framebuffer(&mut spi, &buffer).await.unwrap();
        let top_left = Point::new(160 + 8 * i, 160 + 8 * i);
        let rect = Rectangle::new(top_left, Size::new(80, 80));
        buffer.fill_solid(&rect, BinaryColor::Off).unwrap();
        epd.display_framebuffer(&mut spi, &buffer).await.unwrap();
        Timer::after_secs(5).await;
    }

    let epd = expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    let epd = expect!(epd.power_off().await, "Failed to turn off power to EPD");
    Timer::after_secs(4).await;

    let epd = expect!(epd.power_on().await, "Failed to turn on power to EPD");
    let mut epd = expect!(
        epd.init(&mut spi, RefreshMode::Full).await,
        "Failed to initialize EPD"
    );

    info!("Displaying text with partial refresh over black buffer");
    // Clear first.
    buffer
        .fill_solid(&buffer.bounding_box(), BinaryColor::On)
        .unwrap();
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to display check buffer"
    );
    Timer::after_secs(4).await;

    info!("Changing to partial refresh mode");
    let mut epd = expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Partial).await,
        "Failed to set refresh mode"
    );

    info!("Displaying contrasting text");
    let mut style = TextStyle::default();
    style.alignment = Alignment::Left;
    style.baseline = Baseline::Top;
    let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::Off);
    let text = Text::with_text_style(
        "Playing with contrast",
        Point::new(0, 149),
        character_style,
        style,
    );
    text.draw(&mut buffer).unwrap();
    expect!(
        epd.display_partial_framebuffer(&mut spi, &buffer, text.bounding_box())
            .await,
        "Failed to display text"
    );

    info!("Sleeping EPD");
    let epd = expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    Timer::after_secs(2).await;

    info!("Waking EPD");
    let epd = expect!(epd.wake(&mut spi).await, "Failed to wake EPD");
    Timer::after_secs(1).await;

    // Prepare for border updates. These require full refresh mode.
    let mut epd = expect!(
        epd.init(&mut spi, RefreshMode::Full).await,
        "Failed to initialize EPD"
    );

    // Clear both framebuffers to make the border more obvious.
    buffer.clear(BinaryColor::On).unwrap();
    epd.write_framebuffer(&mut spi, &buffer).await.unwrap();
    info!("Setting white border");
    expect!(
        epd.set_border(&mut spi, BinaryColor::Off).await,
        "Failed to set border color"
    );
    expect!(
        epd.update_display(&mut spi).await,
        "Failed to refresh display"
    );
    Timer::after_secs(5).await;

    // Set old framebuffer to different color to force full-screen update
    // Otherwise the screen performs no update
    buffer.clear(BinaryColor::On).unwrap();
    // epd.write_framebuffer(&mut spi, &buffer).await.unwrap();
    epd.write_base_framebuffer(&mut spi, &buffer).await.unwrap();
    buffer.clear(BinaryColor::Off).unwrap();
    epd.write_framebuffer(&mut spi, &buffer).await.unwrap();
    info!("Setting black border");
    expect!(
        epd.set_border(&mut spi, BinaryColor::On).await,
        "Failed to set border color"
    );
    expect!(
        epd.update_display(&mut spi).await,
        "Failed to refresh display"
    );
    Timer::after_secs(3).await;

    buffer.clear(BinaryColor::Off).unwrap();
    expect!(
        epd.set_border(&mut spi, BinaryColor::On).await,
        "Failed to set border color"
    );
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to refresh display"
    );
    Timer::after_secs(3).await;

    info!("Clearing screen for the final time");
    let mut epd = expect!(
        epd.init(&mut spi, RefreshMode::Full).await,
        "Failed to initialize EPD"
    );
    buffer
        .fill_solid(&buffer.bounding_box(), BinaryColor::Off)
        .unwrap();
    info!("Displaying white buffer");
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to display buffer"
    );
    Timer::after_secs(5).await;
    let epd = expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    epd.power_off().await.unwrap();
    info!("Done");
}
