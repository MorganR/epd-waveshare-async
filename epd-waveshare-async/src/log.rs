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

macro_rules! trace {
    ($($arg:tt)*) => {
        #[cfg(feature = "defmt")]
        defmt::trace!($($arg)*);

        #[cfg(feature = "log")]
        log::trace!($($arg)*);
    };
}

pub(crate) use {debug, error, trace};
