macro_rules! error {
    ($($arg:tt)*) => {
        #[cfg(feature = "defmt")]
        defmt::error!($($arg)*);

        #[cfg(feature = "log")]
        log::error!($($arg)*);
    };
}

macro_rules! debug {
    ($($arg:tt)*) => {
        #[cfg(feature = "defmt")]
        defmt::debug!($($arg)*);

        #[cfg(feature = "log")]
        log::debug!($($arg)*);
    };
}

pub(crate) use {debug, error};