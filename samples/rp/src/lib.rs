#![no_std]

use core::convert::Infallible;

use embassy_embedded_hal::shared_bus::SpiDeviceError;
use embassy_rp::spi;
use thiserror::Error as ThisError;

pub type RawSpiError = SpiDeviceError<spi::Error, Infallible>;
#[derive(Debug, ThisError)]
pub enum Error {
    #[error("SPI error: {0:?}")]
    SpiError(RawSpiError),
}

impl From<Infallible> for Error {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

impl From<RawSpiError> for Error {
    fn from(e: RawSpiError) -> Self {
        Error::SpiError(e)
    }
}
