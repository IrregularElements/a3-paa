#![no_main]
use libfuzzer_sys::fuzz_target;

use std::io::Cursor;

use a3_paa::PaaImage;

fuzz_target!(|data: &[u8]| {
	let mut cursor = Cursor::new(data);
	let image = PaaImage::read_from(&mut cursor);

	if let Ok(image) = image {
		let _ = image.as_bytes();
	};
});
