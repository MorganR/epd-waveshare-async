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

macro_rules! warn_log {
    ($($arg:tt)*) => {
        #[cfg(feature = "defmt")]
        defmt::warn!($($arg)*);

        #[cfg(feature = "log")]
        log::warn!($($arg)*);
    };
}

macro_rules! debug_assert {
    ($assertion:expr, $message:expr) => {
        #[cfg(feature = "defmt")]
        defmt::debug_assert!(
            $assertion,
            $message,
        );
        #[cfg(not(feature = "defmt"))]
        debug_assert!(
            $assertion,
            $message,
        );  
    };
}

pub(crate) use {debug, debug_assert, trace, warn_log};
