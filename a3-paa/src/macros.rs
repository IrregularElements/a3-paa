macro_rules! log {
	($fn:ident, $($arg:tt)*) => {
		#[cfg(feature = "log")]
		log::$fn!($($arg)*);
	}
}

pub(crate) use log;
