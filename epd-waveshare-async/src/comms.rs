use embedded_hal::digital::{InputPin as _, OutputPin as _};
use embedded_hal_async::{digital::Wait as _, spi::SpiDevice as _};

use crate::{log::trace, EpdHw};

/// Provides "wait" support for hardware with a busy state.
pub(crate) trait BusyWait: EpdHw {
    /// Waits for the current operation to complete if the display is busy.
    ///
    /// Note that this will wait forever if the display is asleep.
    async fn wait_if_busy(&mut self) -> Result<(), Self::Error>;
}

/// Provides the ability to send <command> then <data> style communications.
pub(crate) trait CommandDataSend: EpdHw {
    /// Send the following command and data to the display. Waits until the display is no longer busy before sending.
    async fn send(
        &mut self,
        spi: &mut <Self as EpdHw>::Spi,
        command: u8,
        data: &[u8],
    ) -> Result<(), Self::Error>;
}

impl<HW: EpdHw> BusyWait for HW {
    async fn wait_if_busy(&mut self) -> Result<(), HW::Error> {
        let busy = self.busy();
        // Note: the datasheet states that busy pin is active low, i.e. we should wait for it when
        // it's low, but this is incorrect. The sample code treats it as active high, which works.
        if busy.is_high().unwrap() {
            trace!("Waiting for busy EPD");
            busy.wait_for_low().await?;
        }
        Ok(())
    }
}

impl<HW: EpdHw> CommandDataSend for HW {
    async fn send(
        &mut self,
        spi: &mut <Self as EpdHw>::Spi,
        command: u8,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        trace!("Sending EPD command: {:?}", command);
        self.wait_if_busy().await?;

        self.dc().set_low()?;
        spi.write(&[command]).await?;

        if !data.is_empty() {
            self.dc().set_high()?;
            spi.write(data).await?;
        }

        Ok(())
    }
}
