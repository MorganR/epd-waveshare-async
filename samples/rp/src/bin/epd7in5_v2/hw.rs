use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals;
use embassy_rp::spi::{self, Spi};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_time::Delay;
use epd_waveshare_async::epd7in5_v2::Epd7in5v2Hw;
use epd_waveshare_async::EpdHw;
use rp_samples::Error;

// Define the resources needed to communicate with the display.
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
        power: PIN_14,
    },
}

/// Defines the hardware to use for connecting to the display.
pub struct DisplayHw<'a> {
    dc: Output<'a>,
    reset: Output<'a>,
    busy: Input<'a>,
    power: Output<'a>,
    delay: Delay,
}

impl DisplayHw<'_> {
    pub fn new(p: DisplayP) -> Self {
        let dc = Output::new(p.dc, Level::Low);
        let reset = Output::new(p.reset, Level::Low);
        let busy = Input::new(p.busy, Pull::Down);
        let power = Output::new(p.power, Level::Low);

        Self {
            dc,
            reset,
            busy,
            power,
            delay: Delay,
        }
    }
}

type EpdSpiDevice<'a> =
    SpiDevice<'a, NoopRawMutex, Spi<'a, peripherals::SPI1, spi::Async>, Output<'a>>;

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

impl<'a> Epd7in5v2Hw for DisplayHw<'a> {
    type Power = Output<'a>;
    type Error = Error;

    fn power(&mut self) -> &mut Self::Power {
        &mut self.power
    }
}
