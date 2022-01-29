#![no_main]
use libfuzzer_sys::fuzz_target;
use bstr::BString;
use arbitrary::{
	Arbitrary,
	Unstructured,
	Result as ArbitraryResult,
};
use a3_paa::{
	Tagg,
	Transparency,
};


#[derive(Debug, PartialEq)]
pub enum TaggFuzzer {
	Avgc {
		rgba: u32,
	},

	Maxc {
		rgba: u32,
	},

	Flag {
		transparency: TransparencyFuzzer,
	},

	Swiz {
		swizzle: u32,
	},

	Proc {
		text: BStringFuzzer,
	},

	Offs {
		offsets: Vec<u32>,
	},
}


impl From<TaggFuzzer> for Tagg {
	fn from(value: TaggFuzzer) -> Self {
		use TaggFuzzer::*;

		match value {
			Avgc { rgba } => Tagg::Avgc { rgba },
			Maxc { rgba } => Tagg::Maxc { rgba },
			Flag { transparency } => Tagg::Flag { transparency: transparency.into() },
			Swiz { swizzle } => Tagg::Swiz { swizzle },
			Proc { text } => Tagg::Proc { text: text.into() },
			Offs { offsets } => Tagg::Offs { offsets },
		}
	}
}

impl<'a> Arbitrary<'a> for TaggFuzzer {
	fn arbitrary(input: &mut Unstructured) -> ArbitraryResult<Self> {
		use TaggFuzzer::*;

		let variant_idx: usize = input.int_in_range(1..=6)?;

		let result = match variant_idx {
			1 => {
				Avgc { rgba: <u32 as Arbitrary>::arbitrary(input)? }
			},
			2 => {
				Maxc { rgba: <u32 as Arbitrary>::arbitrary(input)? }
			},
			3 => {
				Flag { transparency: <TransparencyFuzzer as Arbitrary>::arbitrary(input)? }
			},
			4 => {
				Swiz { swizzle: <u32 as Arbitrary>::arbitrary(input)? }
			},
			5 => {
				Proc { text: <BStringFuzzer as Arbitrary>::arbitrary(input)? }
			},
			6 => {
				let offs_len = input.int_in_range(0..=16)? as usize;
				let mut offsets: Vec<u32> = vec![0u32; offs_len];

				for o in offsets.iter_mut() {
					*o = <u32 as Arbitrary>::arbitrary(input)?;
				}

				if let Some(idx) = offsets.iter().position(|x| *x == 0) {
					offsets.truncate(idx);
				}

				Offs { offsets }

			},
			_ => panic!(),
		};

		Ok(result)
	}
}



#[derive(Debug, Arbitrary, PartialEq)]
pub enum TransparencyFuzzer {
	None,
	AlphaInterpolated,
	AlphaNotInterpolated,
}


impl From<TransparencyFuzzer> for Transparency {
	fn from(value: TransparencyFuzzer) -> Self {
		use TransparencyFuzzer::*;

		match value {
			None => Transparency::None,
			AlphaInterpolated => Transparency::AlphaInterpolated,
			AlphaNotInterpolated => Transparency::AlphaNotInterpolated,
		}
	}
}



#[derive(Debug, Arbitrary, PartialEq)]
pub struct BStringFuzzer(Vec<u8>);


impl From<BStringFuzzer> for BString {
	fn from(value: BStringFuzzer) -> Self {
		BString::from(value.0)
	}
}



fuzz_target!(|tuple: (TaggFuzzer, &[u8])| {
	use std::convert::TryInto;

	let (tagg, data) = tuple;
	let tagg: Tagg = tagg.into();
	let tagg_name = tagg.as_taggname();
	assert!(Tagg::is_valid_taggname(&tagg_name));

	let bytes = tagg.to_bytes();
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
