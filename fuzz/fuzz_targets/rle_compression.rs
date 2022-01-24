#![no_main]
use libfuzzer_sys::fuzz_target;

use a3_paa::{compress_rleblock_slice, decompress_rleblock_slice};

fuzz_target!(|data: &[u8]| {
	let compressed = compress_rleblock_slice(data);
	let decompressed = decompress_rleblock_slice(&compressed[..]).unwrap();
	assert_eq!(data, decompressed);
});
