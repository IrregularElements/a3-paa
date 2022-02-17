#![allow(deprecated)]

#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::{
	Arbitrary,
	Unstructured,
	Result as ArbitraryResult,
};
use a3_paa::{PaaMipmap, PaaMipmapCompression, PaaType};


#[derive(Debug, Arbitrary)]
pub enum PaaTypeFuzzer {
	Dxt1,
	Dxt2,
	Dxt3,
	Dxt4,
	Dxt5,
	Rgba4,
	Rgba5,
	Rgba8,
	Gray,
	IndexPalette,
}

impl From<&PaaTypeFuzzer> for PaaType {
	fn from(value: &PaaTypeFuzzer) -> Self {
		use PaaTypeFuzzer::*;
		match value {
			Dxt1 => PaaType::Dxt1,
			Dxt2 => PaaType::Dxt2,
			Dxt3 => PaaType::Dxt3,
			Dxt4 => PaaType::Dxt4,
			Dxt5 => PaaType::Dxt5,
			Rgba4 => PaaType::Rgba4,
			Rgba5 => PaaType::Rgba5,
			Rgba8 => PaaType::Rgba8,
			Gray => PaaType::Gray,
			IndexPalette => PaaType::IndexPalette,
		}
	}
}


#[derive(Debug, Copy, Clone, Arbitrary)]
pub enum PaaMipmapCompressionFuzzer {
	Uncompressed,
	Lzo,
	Lzss,
	RleBlocks,
}

impl From<PaaMipmapCompressionFuzzer> for PaaMipmapCompression {
	fn from(value: PaaMipmapCompressionFuzzer) -> Self {
		use PaaMipmapCompressionFuzzer::*;
		match value {
			Uncompressed => PaaMipmapCompression::Uncompressed,
			Lzo => PaaMipmapCompression::Lzo,
			Lzss => PaaMipmapCompression::Lzss,
			RleBlocks => PaaMipmapCompression::RleBlocks,
		}
	}
}


#[derive(Debug)]
struct PaaMipmapFuzzer {
	width: u16,
	height: u16,
	paatype: PaaTypeFuzzer,
	compression: PaaMipmapCompressionFuzzer,
	data: Vec<u8>,
}

impl<'a> Arbitrary<'a> for PaaMipmapFuzzer {
	fn arbitrary(input: &mut Unstructured) -> ArbitraryResult<Self> {
		use PaaTypeFuzzer::*;
		use PaaMipmapCompressionFuzzer::*;

		let paatype = <PaaTypeFuzzer as Arbitrary>::arbitrary(input)?;

		let compression = match &paatype {
			Dxt1 | Dxt2 | Dxt3 | Dxt4 | Dxt5 => Lzo,
			IndexPalette => *input.choose(&[Lzss, RleBlocks])?,
			_ => <PaaMipmapCompressionFuzzer as Arbitrary>::arbitrary(input)?,
		};

		let (width, height) = if PaaType::from(&paatype).is_dxtn() {
			// Real-life PAA-DXT dimension limit is 2^14 (16384), we limit it
			// to 2^10 to avoid slow-unit fuzz artifacts.
			let width: u16 = 2u16.pow(input.int_in_range(2..=10)?);
			let height: u16 = 2u16.pow(input.int_in_range(2..=10)?);

			(width, height)
		}
		else {
			// Real-life PAA (non-DXT) dimension limit is 0xFFFF^0x8000 (32767).
			let width: u16 = input.int_in_range(1..=2000)?;
			let height: u16 = input.int_in_range(1..=2000)?;

			(width, height)
		};

		let data_len = PaaType::from(&paatype).predict_size(width, height);
		let mut data: Vec<u8> = vec![0u8; data_len];
		input.fill_buffer(&mut data)?;

		Ok(Self { width, height, paatype, compression, data })
	}
}

impl From<PaaMipmapFuzzer> for PaaMipmap {
	fn from(value: PaaMipmapFuzzer) -> Self {
		let width = value.width;
		let height = value.height;
		let paatype = (&value.paatype).into();
		let compression = value.compression.into();
		let data = value.data;
		Self { width, height, paatype, compression, data }
	}
}


fuzz_target!(|mip: PaaMipmapFuzzer| {
	let mip: PaaMipmap = mip.into();
	let paatype = mip.paatype;
	let bytes = mip.as_bytes().unwrap();
	let mipp = PaaMipmap::from_bytes(&bytes, paatype).unwrap();
	assert_eq!(mip.width, mipp.width);
	assert_eq!(mip.height, mipp.height);
	assert_eq!(mip.paatype, mipp.paatype);
	assert_eq!(mip.compression, mipp.compression);
	assert_eq!(mip.data, mipp.data);
});
