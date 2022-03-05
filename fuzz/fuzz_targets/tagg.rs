#![no_main]
use libfuzzer_sys::fuzz_target;
use a3_paa::Tagg;


fuzz_target!(|tuple: (Tagg, &[u8])| {
	use std::convert::TryInto;

	let (tagg, data) = tuple;
	let tagg_name = tagg.as_taggname();
	assert!(Tagg::is_valid_taggname(&tagg_name));

	let bytes = tagg.as_bytes();
	let tagg_data = &bytes[12..];

	let tagg_prime = Tagg::from_name_and_payload(&tagg_name, tagg_data).unwrap();
	assert_eq!(tagg, tagg_prime);

	if data.len() < 12 {
		return;
	}

	let tagg_head = &data[0..12];
	let tagg_head: [u8; 12] = tagg_head.try_into().unwrap();
	let tagg_head_result = Tagg::try_head_from(&tagg_head);

	if let Ok((name, payload_size)) = tagg_head_result {
		let payload = &data[12..];

		if payload.len() < payload_size as usize {
			return;
		}

		let payload = &payload[..(payload_size as usize)];

		let _ = Tagg::from_name_and_payload(&name, payload);
	}
	else {
		return;
	}
});
