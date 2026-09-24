//! Bring-up test for the Waveshare 3.5" e-Paper (G) on an STM32F3 Discovery (STM32F303VCT6).
//!
//! SPI1 (PA5-PA7) on this board goes to the on-board L3GD20 gyroscope, so the display uses SPI2
//! and the free port D pins instead. Wiring (module pin -> Discovery pin):
//!
//! | Module | Discovery | Notes                                             |
//! |--------|-----------|---------------------------------------------------|
//! | VCC    | 3V        | Not 5V: the IO level must match VCC               |
//! | GND    | GND       |                                                   |
//! | DIN    | PB15      | SPI2 MOSI                                         |
//! | CLK    | PB13      | SPI2 SCK                                          |
//! | CS     | PB12      |                                                   |
//! | DC     | PD8       |                                                   |
//! | RST    | PD9       |                                                   |
//! | BUSY   | PD10      | EXTI10                                            |
//! | PWR    | PD11      | Only on 9-pin driver boards; switches panel power |
//!
//! Change the pins in `assign_resources!` below to match your wiring. BUSY needs its matching
//! EXTI channel and interrupt (PD10 -> EXTI10, which shares the EXTI15_10 interrupt).
//!
//! Flash from this directory with `cargo run --release --bin epd3in5g`, through the board's
//! on-board probe or any other SWD probe probe-rs supports.
//!
//! Each step logs what the panel should show. A full refresh takes about 18 seconds, with the
//! panel flashing through several colours before settling. The refreshes here are closer together
//! than the 180 s the panel's guidance asks for; that's fine for an occasional test.

#![no_main]
#![no_std]

use assign_resources::assign_resources;
use defmt::{info, unwrap};
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_executor::Spawner;
use embassy_stm32::exti::{self, ExtiInput};
use embassy_stm32::gpio::{Level, Output, Pull, Speed};
use embassy_stm32::mode::Async;
use embassy_stm32::spi::{self, mode::Master, Spi};
use embassy_stm32::time::Hertz;
use embassy_stm32::{bind_interrupts, dma, interrupt, peripherals, Peri};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Instant, Timer};
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::text::{Baseline, Text};
use epd_waveshare_async::epd3in5g::{self, Color, Epd3In5G, Epd3In5GBuffer, InitSequence};
use epd_waveshare_async::{DisplaySimple, Sleep, Wake};
use static_cell::{ConstStaticCell, StaticCell};
use stm32f303_samples::DisplayHw;

/// Change to [InitSequence::Datasheet] to try the datasheet's shorter register sequence.
const INIT_SEQUENCE: InitSequence = InitSequence::Vendor;

/// The framebuffer is 17,664 bytes, so it lives in a static rather than on the stack.
static BUFFER: ConstStaticCell<Epd3In5GBuffer> = ConstStaticCell::new(epd3in5g::new_buffer());

static SPI_BUS: StaticCell<Mutex<NoopRawMutex, Spi<'static, Async, Master>>> = StaticCell::new();

bind_interrupts!(struct Irqs {
    DMA1_CHANNEL5 => dma::InterruptHandler<peripherals::DMA1_CH5>;
    EXTI15_10 => exti::InterruptHandler<interrupt::typelevel::EXTI15_10>;
});

assign_resources! {
    spi: SpiResources {
        peri: SPI2,
        sck: PB13,
        mosi: PB15,
        dma_tx: DMA1_CH5,
        cs: PB12,
    },
    epd: EpdResources {
        dc: PD8,
        reset: PD9,
        busy: PD10,
        busy_exti: EXTI10,
        power: PD11,
    },
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    let r = split_resources!(p);

    let mut config = spi::Config::default();
    config.frequency = Hertz(epd3in5g::RECOMMENDED_SPI_HZ);
    // embassy-stm32 uses the embedded-hal 0.2 phase and polarity enums, so we have to map these.
    config.mode.phase = match epd3in5g::RECOMMENDED_SPI_PHASE {
        embedded_hal::spi::Phase::CaptureOnFirstTransition => spi::Phase::CaptureOnFirstTransition,
        embedded_hal::spi::Phase::CaptureOnSecondTransition => {
            spi::Phase::CaptureOnSecondTransition
        }
    };
    config.mode.polarity = match epd3in5g::RECOMMENDED_SPI_POLARITY {
        embedded_hal::spi::Polarity::IdleHigh => spi::Polarity::IdleHigh,
        embedded_hal::spi::Polarity::IdleLow => spi::Polarity::IdleLow,
    };

    let spi_bus = SPI_BUS.init(Mutex::new(Spi::new_txonly(
        r.spi.peri,
        r.spi.sck,
        r.spi.mosi,
        r.spi.dma_tx,
        Irqs,
        config,
    )));
    // CS is active low.
    let cs = Output::new(r.spi.cs, Level::High, Speed::VeryHigh);
    let mut spi = SpiDevice::new(spi_bus, cs);

    // Newer driver boards gate the panel's supply with a PWR pin, which Waveshare's demos drive
    // high before anything else. On 8-pin boards nothing is connected to PD11.
    let mut power = Output::new(r.epd.power, Level::High, Speed::Low);

    // The module drives BUSY, so no pull is needed.
    let busy = ExtiInput::new(r.epd.busy, r.epd.busy_exti, Pull::None, Irqs);
    let hw = DisplayHw::new(r.epd.dc, r.epd.reset, busy, epd3in5g::DEFAULT_BUSY_WHEN);

    info!("Initialising EPD with the {} sequence", INIT_SEQUENCE);
    let mut epd = unwrap!(Epd3In5G::new(hw).init(&mut spi, INIT_SEQUENCE).await);

    let buffer = BUFFER.take();

    info!("Step 1: colour bands");
    info!("  Expect four horizontal bands, top to bottom: black, white, yellow, red.");
    info!("  Each band is labelled with its colour name.");
    info!("  A yellow square and 'TOP LEFT' mark pixel (0, 0).");
    draw_colour_bands(buffer);
    let start = Instant::now();
    unwrap!(epd.display_framebuffer(&mut spi, buffer).await);
    info!("  Refresh took {} ms", start.elapsed().as_millis());

    info!("Step 2: deep sleep for 10 s. The image should stay on the panel.");
    let epd = unwrap!(epd.sleep(&mut spi).await);
    Timer::after_secs(10).await;

    info!("Step 3: wake, red border");
    info!("  Expect white with 'Woke from sleep' in black and 'Red border' in red,");
    info!("  and a red frame around the active area.");
    let mut epd = unwrap!(epd.wake(&mut spi).await);
    unwrap!(epd.set_border(&mut spi, Color::Red).await);
    buffer.clear(Color::White).unwrap();
    draw_text(buffer, "Woke from sleep", Point::new(8, 8), Color::Black);
    draw_text(buffer, "Red border", Point::new(8, 24), Color::Red);
    unwrap!(epd.display_framebuffer(&mut spi, buffer).await);
    Timer::after_secs(10).await;

    info!("Step 4: shut down");
    info!("  Expect all white with a white border: the datasheet's state for storage.");
    let _epd = unwrap!(epd.shutdown(&mut spi).await);
    // Waveshare's demos cut panel power on exit. Safe now the panel is white and asleep.
    power.set_low();
    info!("Done. Safe to remove power.");
}

fn draw_colour_bands(buffer: &mut Epd3In5GBuffer) {
    let size = buffer.bounding_box().size;
    let band_height = size.height / 4;
    let bands = [
        (Color::Black, "BLACK", Color::White),
        (Color::White, "WHITE", Color::Black),
        (Color::Yellow, "YELLOW", Color::Black),
        (Color::Red, "RED", Color::White),
    ];
    for (i, (background, name, text_colour)) in bands.into_iter().enumerate() {
        let top = (i as u32 * band_height) as i32;
        buffer
            .fill_solid(
                &Rectangle::new(Point::new(0, top), Size::new(size.width, band_height)),
                background,
            )
            .unwrap();
        draw_text(buffer, name, Point::new(8, top + 44), text_colour);
    }

    buffer
        .fill_solid(
            &Rectangle::new(Point::zero(), Size::new(16, 16)),
            Color::Yellow,
        )
        .unwrap();
    draw_text(buffer, "TOP LEFT", Point::new(20, 3), Color::White);
}

fn draw_text(buffer: &mut Epd3In5GBuffer, text: &str, position: Point, colour: Color) {
    let style = MonoTextStyle::new(&FONT_6X10, colour);
    Text::with_baseline(text, position, style, Baseline::Top)
        .draw(buffer)
        .unwrap();
}
