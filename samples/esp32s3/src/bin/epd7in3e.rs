#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use core::mem::MaybeUninit;
use core::ptr::NonNull;

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Delay, Timer};
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::pixelcolor::Rgb888;
use embedded_graphics::primitives::Rectangle;
use embedded_hal_bus::spi::ExclusiveDevice;
use epd_waveshare_async::buffer::{HexBuffer, HexColor};
use epd_waveshare_async::epd7in3e::{self, Epd7In3E, Epd7In3EBuffer, RefreshMode};
use epd_waveshare_async::{DisplaySimple, Reset, Sleep};
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::spi::master::{Config, Spi};
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{Async, peripherals::*};
use esp_println as _;
use esp32s3::{
    DisplayHw, DisplayResources, EpdResources, Resources, RtosResources, split_resources,
};

#[panic_handler]
fn panic(panic_info: &core::panic::PanicInfo) -> ! {
    error!("{}", panic_info);
    loop {}
}

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

static mut EPD_BUFFER: MaybeUninit<Epd7In3EBuffer> = MaybeUninit::uninit();

type EpdSpi<'a> = ExclusiveDevice<Spi<'a, Async>, Output<'a>, Delay>;

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(_spawner: Spawner) -> ! {
    // generator version: 1.3.0
    // generator parameters: --chip esp32s3 -o unstable-hal -o embassy -o defmt -o esp

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let r = split_resources!(peripherals);

    let timg = TimerGroup::new(r.rtos.timg);
    let sw_interrupt = esp_hal::interrupt::software::SoftwareInterruptControl::new(r.rtos.swint);
    esp_rtos::start(timg.timer0, sw_interrupt.software_interrupt0);

    info!("Embassy initialized!");

    let r_disp = r.display;
    let r_epd = r.epd;

    let delay = Delay;

    let bus = Spi::new(r_disp.spi, Config::default())
        .unwrap()
        .with_mosi(r_disp.mosi)
        .with_sck(r_disp.clk)
        .into_async();
    let cs = Output::new(r_disp.cs, Level::High, OutputConfig::default());

    let mut spi_device = ExclusiveDevice::new(bus, cs, delay).unwrap();

    defmt::info!("SPI setup successful");

    #[expect(static_mut_refs)]
    unsafe {
        HexBuffer::init(
            NonNull::new(EPD_BUFFER.as_mut_ptr()).expect("EPD_BUFFER should be allocated"),
            Size::new(epd7in3e::DISPLAY_WIDTH, epd7in3e::DISPLAY_HEIGHT),
        );
    }

    // Setup EPD
    let epd_uninit = Epd7In3E::new(DisplayHw::<EpdSpi>::new::<GPIO10, GPIO38, GPIO4>(
        r_epd.dc,
        r_epd.rst,
        r_epd.busy,
        epd7in3e::DEFAULT_BUSY_WHEN,
    ));

    let mut epd = epd_uninit
        .init(&mut spi_device, RefreshMode::Full)
        .await
        .unwrap();

    let color_6 = [
        HexColor::Black,
        HexColor::White,
        HexColor::Yellow,
        HexColor::Red,
        HexColor::Blue,
        HexColor::Green,
    ];

    #[expect(static_mut_refs)]
    unsafe {
        let buffer = EPD_BUFFER.assume_init_mut();

        for (i, color) in color_6.iter().enumerate() {
            let w = epd7in3e::DISPLAY_WIDTH;
            let h = epd7in3e::DISPLAY_HEIGHT/(color_6.len() as u32);
            let y = i as u32 * h;
            let rect = Rectangle::new(Point::new(0, y.cast_signed()), Size::new(w, h));
            buffer.fill_solid(&rect, Rgb888::from(*color));
        }
    }


    defmt::debug!("Starting EPD update...");
    #[expect(static_mut_refs)]
    unsafe {
        epd.display_framebuffer(&mut spi_device, EPD_BUFFER.assume_init_ref())
            .await
            .unwrap();
    }
    let epd_asleep = defmt::expect!(
        epd.sleep(&mut spi_device).await,
        "Failed to display text buffer"
    );

    defmt::debug!("EPD done.");
    // for inspiration have a look at the examples at https://github.com/esp-rs/esp-hal/tree/esp-hal-v1.1.0/examples

    loop {
        Timer::after_secs(1).await;
    }
}
