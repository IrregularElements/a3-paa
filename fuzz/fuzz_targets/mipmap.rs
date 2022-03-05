#![allow(deprecated)]

#![no_main]
use libfuzzer_sys::fuzz_target;
use a3_paa::PaaMipmap;


fuzz_target!(|mip: PaaMipmap| {
	let paatype = mip.paatype;
	let bytes = mip.as_bytes().unwrap();
	let mipp = PaaMipmap::from_bytes(&bytes, paatype).unwrap();
	assert_eq!(mip.width, mipp.width);
	assert_eq!(mip.height, mipp.height);
	assert_eq!(mip.paatype, mipp.paatype);
	assert_eq!(mip.compression, mipp.compression);
	assert_eq!(mip.data, mipp.data);
});
