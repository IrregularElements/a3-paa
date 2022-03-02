use crate::*;
use crate::pixconv::*;
use crate::PaaError::*;

use image::{RgbaImage, Pixel};


pub struct PaaDecoder {
	paa: PaaImage,
}


impl PaaDecoder {
	pub fn from_paa(paa: PaaImage) -> Self {
		Self { paa }
	}


	pub fn decode_nth(&self, index: usize) -> PaaResult<RgbaImage> {
		let mipmap = match &self.paa.mipmaps {
			PaaMipmapContainer::Fallible(v) => {
				v.get(index)
					.ok_or(MipmapIndexOutOfRange)?
					.as_ref()
					.map_err(|e| e.clone())?
			},

			PaaMipmapContainer::Infallible(v) => {
				v.get(index)
					.ok_or(MipmapIndexOutOfRange)?
			},
		};

		decode_mipmap(mipmap)
	}


	pub fn decode_first(&self) -> PaaResult<RgbaImage> {
		self.decode_nth(0)
	}
}


fn decode_mipmap(mipmap: &PaaMipmap) -> PaaResult<RgbaImage> {
	use PaaType::*;

	if mipmap.is_empty() {
		return Err(EmptyMipmap);
	};

	match mipmap.paatype {
		paatype @ (Dxt1 | Dxt2 | Dxt3 | Dxt4 | Dxt5) => {
			let (comp_ratio, format) = match &paatype {
				Dxt1 => (8, squish::Format::Bc1),
				Dxt2 => (4, squish::Format::Bc2),
				Dxt3 => (4, squish::Format::Bc2),
				Dxt4 => (4, squish::Format::Bc3),
				Dxt5 => (4, squish::Format::Bc3),
				_ => unreachable!(),
			};

			let mut buffer = vec![0u8; mipmap.data.len() * comp_ratio];
			format.decompress(&mipmap.data, mipmap.width.into(), mipmap.height.into(), &mut buffer);

			let image = RgbaImage::from_vec(mipmap.width.into(), mipmap.height.into(), buffer).unwrap();
			Ok(image)
		},

		Argb4444 => {
			let data = argb4444_to_rgba8888(&mipmap.data);
			let image = RgbaImage::from_vec(mipmap.width.into(), mipmap.height.into(), data).unwrap();
			Ok(image)
		},

		Argb1555 => {
			let data = argb1555_to_rgba8888(&mipmap.data);
			let image = RgbaImage::from_vec(mipmap.width.into(), mipmap.height.into(), data).unwrap();
			Ok(image)
		},

		Argb8888 => {
			let data = argb8888_to_rgba8888(&mipmap.data);
			let image = RgbaImage::from_vec(mipmap.width.into(), mipmap.height.into(), data).unwrap();
			Ok(image)
		},

		_ => todo!(),
	}
}
