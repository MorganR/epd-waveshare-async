use core::error::Error as CoreError;

use embedded_hal::{digital::{ErrorType as PinErrorType, InputPin, OutputPin}, spi::ErrorType as SpiErrorType};
use embedded_hal_async::{delay::DelayNs, digital::Wait, spi::SpiDevice};

use crate::log::trace;

/// Provides access to the hardware needed to control an EPD.
///
/// This greatly simplifies the generics needed by the `Epd` trait and implementing types at the cost of implementing this trait.
///
/// In this example, we make the EPD generic over just the SPI type, but can drop generics for the pins and delay type.
///
/// ```rust
/// use core::convert::Infallible;
///
/// use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
/// use embassy_embedded_hal::shared_bus::SpiDeviceError;
/// use embassy_rp::gpio::{Input, Output};
/// use embassy_rp::spi::{self, Spi};
/// use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
/// use embassy_time::Delay;
/// use epd_waveshare_async::EpdHw;
/// use thiserror::Error as ThisError;
///
/// /// Define an error type that can convert from the SPI and GPIO errors.
/// #[derive(Debug, ThisError)]
/// enum Error {
///   #[error("SPI error: {0:?}")]
///   SpiError(SpiDeviceError<spi::Error, Infallible>),
/// }
///
/// impl From<Infallible> for Error {
///     fn from(_: Infallible) -> Self {
///         // GPIO errors are infallible, i.e. they can't occur, so this should be unreachable.
///         unreachable!()
///     }
/// }
///
/// impl From<SpiDeviceError<spi::Error, Infallible>> for Error {
///     fn from(e: SpiDeviceError<spi::Error, Infallible>) -> Self {
///         Error::SpiError(e)
///     }
/// }
///
/// struct RpEpdHw<'a, SPI: spi::Instance + 'a> {
///     dc: Output<'a>,
///     reset: Output<'a>,
///     busy: Input<'a>,
///     delay: Delay,
///     _phantom: core::marker::PhantomData<SPI>,
/// }
///
/// impl <'a, SPI: spi::Instance + 'a> EpdHw for RpEpdHw<'a, SPI> {
///     type Spi = SpiDevice<'a, CriticalSectionRawMutex, Spi<'a, SPI, spi::Async>, Output<'a>>;
///     type Dc = Output<'a>;
///     type Reset = Output<'a>;
///     type Busy = Input<'a>;
///     type Delay = Delay;
///     type Error = Error;
///
///     fn dc(&mut self) -> &mut Self::Dc {
///       &mut self.dc
///     }
///
///     fn reset(&mut self) -> &mut Self::Reset {
///       &mut self.reset
///     }
///
///     fn busy(&mut self) -> &mut Self::Busy {
///       &mut self.busy
///     }
///
///     fn delay(&mut self) -> &mut Self::Delay {
///       &mut self.delay
///     }
/// }
/// ```
pub trait EpdHw {
    type Spi: SpiDevice;
    type Dc: OutputPin;
    type Reset: OutputPin;
    type Busy: InputPin + Wait;
    type Delay: DelayNs;
    type Error: CoreError
        + From<<Self::Spi as SpiErrorType>::Error>
        + From<<Self::Dc as PinErrorType>::Error>
        + From<<Self::Reset as PinErrorType>::Error>
        + From<<Self::Busy as PinErrorType>::Error>;

    fn dc(&mut self) -> &mut Self::Dc;
    fn reset(&mut self) -> &mut Self::Reset;
    fn busy(&mut self) -> &mut Self::Busy;
    fn delay(&mut self) -> &mut Self::Delay;
}

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
