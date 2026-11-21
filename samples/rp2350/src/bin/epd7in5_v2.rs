//! This example tests the EPD Waveshare 7.5" v2 display driver using a Waveshare RP2350-ETH board.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

use defmt::{expect, info, Debug2Format, Display2Format};
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_executor::Spawner;
use embassy_rp::dma;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::interrupt::Interrupt::DMA_IRQ_1;
use embassy_rp::peripherals::{DMA_CH0, DMA_CH1, PIO0, UART0, UART1};
use embassy_rp::pio;
use embassy_rp::spi::{self, Spi};
use embassy_rp::uart::{self, Blocking, BufferedInterruptHandler, Uart};
use embassy_rp::Peri;
use embassy_rp::{bind_interrupts, peripherals};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Instant, Timer};
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::{BinaryColor, Gray2};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyle};
use epd_waveshare_async::epd7in5_v2::{self, new_gray2_buffer, Epd7In5V2, RefreshMode};
use epd_waveshare_async::{Clear, DisplayPartial, DisplaySimple, Displayable, Reset, Sleep, Wake};
use rp2350_samples::*;
use static_cell::StaticCell;

// Define the resources needed to communicate with the display.
assign_resources::assign_resources! {
    spi_hw: SpiP {
        spi: SPI0,
        clk: PIN_2,
        tx: PIN_3,
        dma_tx: DMA_CH0,
        cs: PIN_5,
    },
    epd_hw: DisplayP {
        reset: PIN_7,
        dc: PIN_6,
        busy: PIN_8,
        pwr:PIN_9
    },
}

#[panic_handler]
pub fn panic(info: &PanicInfo) -> ! {
    defmt::error!("{}", Display2Format(info));
    loop {}
}

#[cortex_m_rt::exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    defmt::error!("HardFault: {:?}", Debug2Format(ef));
    loop {}
}

static SERIAL: StaticCell<Uart<'static, Blocking>> = StaticCell::new();

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>;
    UART0_IRQ => BufferedInterruptHandler<UART0>;
    UART1_IRQ => BufferedInterruptHandler<UART1>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let (tx_pin, rx_pin, uart) = (p.PIN_0, p.PIN_1, p.UART0);
    let uart = uart::Uart::new_blocking(uart, tx_pin, rx_pin, uart::Config::default());
    defmt_serial::defmt_serial(SERIAL.init(uart));
    defmt::info!("Defmt OK");

    let resources = split_resources!(p);
    let mut config = spi::Config::default();
    config.frequency = epd7in5_v2::RECOMMENDED_SPI_HZ;
    // embassy-rp uses the synchronous phase and polarity enums, so we have to map these.
    config.phase = match epd7in5_v2::RECOMMENDED_SPI_PHASE {
        embedded_hal_async::spi::Phase::CaptureOnFirstTransition => {
            embassy_rp::spi::Phase::CaptureOnFirstTransition
        }
        embedded_hal_async::spi::Phase::CaptureOnSecondTransition => {
            embassy_rp::spi::Phase::CaptureOnSecondTransition
        }
    };
    config.polarity = match epd7in5_v2::RECOMMENDED_SPI_POLARITY {
        embedded_hal_async::spi::Polarity::IdleHigh => embassy_rp::spi::Polarity::IdleHigh,
        embedded_hal_async::spi::Polarity::IdleLow => embassy_rp::spi::Polarity::IdleLow,
    };

    let raw_spi: Mutex<NoopRawMutex, _> = Mutex::new(Spi::new_txonly(
        resources.spi_hw.spi,
        resources.spi_hw.clk,
        resources.spi_hw.tx,
        resources.spi_hw.dma_tx,
        Irqs,
        config,
    ));
    // CS is active low.
    let cs_pin = Output::new(resources.spi_hw.cs, Level::High);
    let _pwr = Output::new(resources.epd_hw.pwr, Level::High);

    let mut spi = SpiDevice::new(&raw_spi, cs_pin);
    let epd = Epd7In5V2::new(DisplayHw::new(
        resources.epd_hw.dc,
        resources.epd_hw.reset,
        resources.epd_hw.busy,
        epd7in5_v2::DEFAULT_BUSY_WHEN,
    ));

    info!("Resetting EPD");
    let epd = expect!(epd.reset().await, "Failed to reset EPD");

    info!("Initializing EPD");
    let mut epd = expect!(
        epd.init(&mut spi, RefreshMode::Full).await,
        "Failed to initialize EPD"
    );

    let mut buffer = epd7in5_v2::new_binary_buffer();
    buffer
        .fill_solid(&buffer.bounding_box(), BinaryColor::On)
        .unwrap();
    info!("Displaying white buffer");
    expect!(
        epd.display_framebuffer(&mut spi, &buffer).await,
        "Failed to display buffer"
    );
    Timer::after_secs(4).await;

    epd.set_refresh_mode(&mut spi, RefreshMode::Fast)
        .await
        .unwrap();

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
    Timer::after_secs(5).await;

    info!("Displaying check buffer");
    let before_buffer_draw = Instant::now();
    // Clear first.
    buffer
        .fill_solid(&buffer.bounding_box(), BinaryColor::On)
        .unwrap();
    let max_width = buffer.bounding_box().size.width;
    let max_height = buffer.bounding_box().size.height;
    let mut top_left = Point::new(0, 0);
    let mut box_size = 192;
    let mut color = BinaryColor::Off;
    while box_size > 0 {
        while (top_left.x as u32) + box_size < max_width {
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
        color = BinaryColor::Off;
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

    info!("Clearing screen");
    epd.clear(&mut spi).await.unwrap();
    Timer::after_secs(5).await;

    info!("Changing to partial refresh mode");
    expect!(
        epd.set_refresh_mode(&mut spi, RefreshMode::Partial).await,
        "Failed to set refresh mode"
    );

    info!("Displaying black text on white");
    // The first partial refresh uses inverted colors for some reason
    buffer.clear(BinaryColor::Off).unwrap();
    epd.write_base_framebuffer(&mut spi, &buffer).await.unwrap();
    let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    let text = Text::with_text_style(
        "Black text",
        Point::new(10, 30),
        character_style,
        text_style,
    );
    text.draw(&mut buffer).unwrap();
    epd.write_framebuffer(&mut spi, &buffer).await.unwrap();
    epd.update_display(&mut spi).await.unwrap();
    Timer::after_secs(2).await;

    info!("Sleeping EPD");
    let epd = expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    Timer::after_secs(2).await;

    info!("Waking EPD");
    let mut epd = expect!(
        epd.reset()
            .await
            .unwrap()
            .init(&mut spi, RefreshMode::Partial)
            .await,
        "Failed to wake EPD"
    );
    Timer::after_secs(1).await;

    info!("Displaying text");
    buffer.clear(BinaryColor::On).unwrap();
    let character_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::Off);
    let text = Text::with_text_style(
        "I'm awake!",
        Point::new(10, 30),
        character_style,
        text_style,
    );
    text.draw(&mut buffer).unwrap();
    epd.write_framebuffer(&mut spi, &buffer).await.unwrap();
    epd.update_display(&mut spi).await.unwrap();
    Timer::after_secs(3).await;

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
    epd.write_framebuffer(&mut spi, &buffer).await.unwrap();
    epd.update_display(&mut spi).await.unwrap();
    Timer::after_secs(2).await;

    info!("Display 4-color grayscale");
    let mut gray_buffer = new_gray2_buffer();
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
    Timer::after_secs(5).await;

    info!("Final clear");
    expect!(epd.clear(&mut spi).await, "Failed to clear display");
    Timer::after_secs(4).await;

    let _epd = expect!(epd.sleep(&mut spi).await, "Failed to put EPD to sleep");
    info!("Done");
}
