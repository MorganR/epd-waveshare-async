use embedded_hal::{
    digital::{ErrorType as PinErrorType, InputPin, OutputPin, PinState},
    spi::ErrorType as SpiErrorType,
};
use embedded_hal_async::{delay::DelayNs, digital::Wait, spi::SpiDevice};

use crate::log::trace;

/// Provides access to a shared error type.
///
/// Drivers rely on this trait to provide a single Error type that supports [From] conversions
/// from all the hardware-specific error types.
pub trait ErrorHw {
    type Error;
}

/// Describes the SPI hardware to use for interacting with the EPD.
pub trait SpiHw {
    type Spi: SpiDevice;
}

/// Provides access to the Data/Command pin for EPD control.
pub trait DcHw {
    type Dc: OutputPin;

    fn dc(&mut self) -> &mut Self::Dc;
}

/// Provides access to the Reset pin for EPD control.
pub trait ResetHw {
    type Reset: OutputPin;

    fn reset(&mut self) -> &mut Self::Reset;
}

/// Provides access to the Busy pin for EPD status monitoring.
pub trait BusyHw {
    type Busy: InputPin + Wait;

    fn busy(&mut self) -> &mut Self::Busy;

    /// Indicates which state of the busy pin indicates that it's busy.
    ///
    /// This is user-configurable, rather than enforced by the display driver, to allow the user to
    /// use more unexpected wiring configurations.
    fn busy_when(&self) -> embedded_hal::digital::PinState;
}

/// Provides access to delay functionality for EPD timing control.
pub trait DelayHw {
    type Delay: DelayNs;

    fn delay(&mut self) -> &mut Self::Delay;
}

/// Provides "wait" support for hardware with a busy state.
pub(crate) trait BusyWait: ErrorHw {
    /// Waits for the current operation to complete if the display is busy.
    ///
    /// Note that this will wait forever if the display is asleep.
    async fn wait_if_busy(&mut self) -> Result<(), Self::Error>;
}

/// Provides the ability to send <command> then <data> style communications.
pub(crate) trait CommandDataSend: SpiHw + ErrorHw {
    /// Send the following command and data to the display. Waits until the display is no longer busy before sending.
    async fn send(
        &mut self,
        spi: &mut Self::Spi,
        command: u8,
        data: &[u8],
    ) -> Result<(), Self::Error>;
}

impl<HW> BusyWait for HW
where
    HW: BusyHw + ErrorHw,
    <HW as ErrorHw>::Error: From<<HW::Busy as PinErrorType>::Error>,
{
    async fn wait_if_busy(&mut self) -> Result<(), HW::Error> {
        let busy_when = self.busy_when();
        let busy = self.busy();
        match busy_when {
            PinState::High => {
                if busy.is_high()? {
                    trace!("Waiting for busy EPD");
                    busy.wait_for_low().await?;
                }
            }
            PinState::Low => {
                if busy.is_low()? {
                    trace!("Waiting for busy EPD");
                    busy.wait_for_high().await?;
                }
            }
        };
        Ok(())
    }
}

impl<HW> CommandDataSend for HW
where
    HW: DcHw + BusyHw + BusyWait + SpiHw + ErrorHw,
    HW::Error: From<<HW::Spi as SpiErrorType>::Error>
        + From<<HW::Dc as PinErrorType>::Error>
        + From<<HW::Busy as PinErrorType>::Error>,
{
    async fn send(
        &mut self,
        spi: &mut Self::Spi,
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
