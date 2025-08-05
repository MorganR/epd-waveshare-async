use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals;
use embassy_rp::spi::{self, Spi};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_time::Delay;
use epd_waveshare_async::EpdHw;
use rp_samples::Error;

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
    }
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
        let dc = Output::new(p.dc, Level::Low);
        let reset = Output::new(p.reset, Level::Low);
        let busy = Input::new(p.busy, Pull::Down);

        Self {
            dc,
            reset,
            busy,
            delay: Delay,
        }
    }
}

type EpdSpiDevice<'a> =
    SpiDevice<'a, NoopRawMutex, Spi<'a, peripherals::SPI0, spi::Async>, Output<'a>>;

impl<'a> EpdHw for DisplayHw<'a> {
    type Spi = EpdSpiDevice<'a>;

    type Dc = Output<'a>;

    type Reset = Output<'a>;

    type Busy = Input<'a>;

    type Delay = Delay;

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
