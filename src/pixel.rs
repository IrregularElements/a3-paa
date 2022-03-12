use crate::PaaResult;
use crate::PaaError::*;


use deku::{prelude::*, DekuContainerRead, DekuContainerWrite};
use surety::Ensure;
use tap::prelude::*;


#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::integer_arithmetic)]
pub(crate) trait ArgbPixel: for<'a> DekuContainerRead<'a> + DekuContainerWrite + Sized {
	const ALPHA_WIDTH: u8;
	const COLOR_WIDTH: u8;
	const NEEDS_LE_BYTES: bool;

	fn a(&self) -> u8;
	fn r(&self) -> u8;
	fn g(&self) -> u8;
	fn b(&self) -> u8;
	fn from_rgba(rgba: [u8; 4]) -> Self;


	const PIXEL_WIDTH: usize = Self::ALPHA_WIDTH as usize + (Self::COLOR_WIDTH as usize) * 3;
	const PIXEL_WIDTH_BYTES: usize = (Self::PIXEL_WIDTH + 7) / 8;


	fn uint_range(width: u8) -> u8 { (2u16.pow(width.into()) - 1) as u8 }
	fn alpha_range() -> u8 { Self::uint_range(Self::ALPHA_WIDTH) }
	fn color_range() -> u8 { Self::uint_range(Self::COLOR_WIDTH) }


	fn from_data(data: &[u8]) -> PaaResult<Self> {
		let mut data = data.get(0..Self::PIXEL_WIDTH_BYTES)
			.ok_or(PixelReadError)?
			.to_owned();

		if Self::NEEDS_LE_BYTES {
			data.reverse();
		};

		let (_, result) = <Self as DekuContainerRead>::from_bytes((&data, 0))
			.map_err(|_| PixelReadError)?;
		Ok(result)
	}


	fn to_data(&self) -> PaaResult<Vec<u8>> {
		let mut result = <Self as DekuContainerWrite>::to_bytes(self)
			.map_err(|_| PixelReadError)?;

		if Self::NEEDS_LE_BYTES {
			result.reverse();
		};

		Ok(result)
	}


	fn convert_u8(value: u8, from_width: u8, into_width: u8) -> u8 {
		let range_from = Self::uint_range(from_width) as u16;
		let range_into = Self::uint_range(into_width) as u16;
		let bias = range_from / 2; // needed for symmetry
		(((value as u16) * range_into + bias) / range_from) as u8
	}


	fn into_rgba8(self) -> image::Rgba<u8> {
		let r = Self::convert_u8(self.r(), Self::COLOR_WIDTH, 8);
		let g = Self::convert_u8(self.g(), Self::COLOR_WIDTH, 8);
		let b = Self::convert_u8(self.b(), Self::COLOR_WIDTH, 8);
		let a = Self::convert_u8(self.a(), Self::ALPHA_WIDTH, 8);
		image::Rgba::<u8>([r, g, b, a])
	}


	#[inline]
	fn convert_data_into_rgba8_data(data: &[u8]) -> [u8; 4] {
		let pix = Self::from_data(data).unwrap();
		let rgba = pix.into_rgba8();
		rgba.0
	}


	fn from_rgba8(rgba8: &image::Rgba<u8>) -> Self {
		let r = Self::convert_u8(rgba8.0[0], 8, Self::COLOR_WIDTH);
		let g = Self::convert_u8(rgba8.0[1], 8, Self::COLOR_WIDTH);
		let b = Self::convert_u8(rgba8.0[2], 8, Self::COLOR_WIDTH);
		let a = Self::convert_u8(rgba8.0[3], 8, Self::ALPHA_WIDTH);
		Self::from_rgba([r, g, b, a])
	}


	fn display(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		let a = self.a() as f32 / Self::alpha_range() as f32;
		let r = self.r() as f32 / Self::color_range() as f32;
		let g = self.g() as f32 / Self::color_range() as f32;
		let b = self.b() as f32 / Self::color_range() as f32;
		write!(f, "<a={:.3}> <r={:.3}> <g={:.3}> <b={:.3}>", a, r, g, b)
	}


	fn convert_from_rgba8_slice(data: &[u8]) -> PaaResult<Vec<u8>> {
		if data.len() % 4 != 0 {
			return Err(PixelReadError);
		};

		let result_len: usize = (data.len().checked() / 4 * Self::PIXEL_WIDTH_BYTES)
			.ok_or(ArithmeticOverflow)?;
		let mut result = Vec::with_capacity(result_len);

		for pixdata in data.chunks(4).map(|s| s.try_into().unwrap()) {
			let rgba = image::Rgba::<u8>(pixdata);
			let pix = Self::from_rgba8(&rgba);
			let bytes = pix.to_data().unwrap();
			result.extend(&bytes);
		};

		Ok(result)
	}


	fn convert_to_rgba8_slice(data: &[u8]) -> PaaResult<Vec<u8>> {
		if data.len() % Self::PIXEL_WIDTH_BYTES != 0 {
			return Err(PixelReadError);
		};

		let result_len: usize = (data.len().checked() / Self::PIXEL_WIDTH_BYTES * 4)
			.ok_or(ArithmeticOverflow)?;
		let mut result = Vec::with_capacity(result_len);

		for pixdata in data.chunks(Self::PIXEL_WIDTH_BYTES) {
			result.extend(Self::convert_data_into_rgba8_data(pixdata));
		};

		Ok(result)
	}
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite)]
pub(crate) struct Argb1555Pixel {
	#[deku(bits = "1")]
	a: u8,
	#[deku(bits = "5")]
	r: u8,
	#[deku(bits = "5")]
	g: u8,
	#[deku(bits = "5")]
	b: u8,
}


impl ArgbPixel for Argb1555Pixel {
	const ALPHA_WIDTH: u8 = 1;
	const COLOR_WIDTH: u8 = 5;
	const NEEDS_LE_BYTES: bool = true;

	fn a(&self) -> u8 { self.a }
	fn r(&self) -> u8 { self.r }
	fn g(&self) -> u8 { self.g }
	fn b(&self) -> u8 { self.b }


	fn from_rgba(rgba: [u8; 4]) -> Self {
		let r = rgba[0];
		let g = rgba[1];
		let b = rgba[2];
		let a = rgba[3];
		Self { a, r, g, b }
	}


	#[inline]
	fn convert_data_into_rgba8_data(data: &[u8]) -> [u8; 4] {
		let pixel = data.to_owned().tap_mut(|d| d.reverse());

		let a: u8 = pixel[0] >> 7;
		let r: u8 = (pixel[0] >> 2) & 0x1F;
		let g: u8 = (pixel[0] << 3 | pixel[1] >> 5) & 0x1F;
		let b: u8 = pixel[1] & 0x1F;

		let r: u8 = ((u16::from(r) * 0xFF + 0xF) / 0x1F) as u8;
		let g: u8 = ((u16::from(g) * 0xFF + 0xF) / 0x1F) as u8;
		let b: u8 = ((u16::from(b) * 0xFF + 0xF) / 0x1F) as u8;
		let a: u8 = a * 0xFF;

		[r, g, b, a]
	}
}


impl std::fmt::Display for Argb1555Pixel {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		self.display(f)
	}
}


#[test]
fn argb1555pixel_bytes() {
	let purple_rgba = vec![0x6B, 0x00, 0x94, 0xFF];
	let purple_1555 = vec![0x12, 0xB4];
	assert_eq!(Argb1555Pixel::convert_from_rgba8_slice(&purple_rgba).unwrap(), purple_1555);
	assert_eq!(Argb1555Pixel::convert_to_rgba8_slice(&purple_1555).unwrap(), purple_rgba);

	let manual_1555 = vec![0x12, 0x34];
	let manual_rgba = vec![0x6B, 0x00, 0x94, 0x00];
	assert_eq!(Argb1555Pixel::convert_from_rgba8_slice(&manual_rgba).unwrap(), manual_1555);
	assert_eq!(Argb1555Pixel::convert_to_rgba8_slice(&manual_1555).unwrap(), manual_rgba);
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite)]
pub(crate) struct Argb4444Pixel {
	#[deku(bits = "4")]
	a: u8,
	#[deku(bits = "4")]
	r: u8,
	#[deku(bits = "4")]
	g: u8,
	#[deku(bits = "4")]
	b: u8,
}


impl ArgbPixel for Argb4444Pixel {
	const ALPHA_WIDTH: u8 = 4;
	const COLOR_WIDTH: u8 = 4;
	const NEEDS_LE_BYTES: bool = true;

	fn a(&self) -> u8 { self.a }
	fn r(&self) -> u8 { self.r }
	fn g(&self) -> u8 { self.g }
	fn b(&self) -> u8 { self.b }


	fn from_rgba(rgba: [u8; 4]) -> Self {
		let r = rgba[0];
		let g = rgba[1];
		let b = rgba[2];
		let a = rgba[3];
		Self { a, r, g, b }
	}


	#[inline]
	fn convert_data_into_rgba8_data(data: &[u8]) -> [u8; 4] {
		let pixel = data.to_owned().tap_mut(|d| d.reverse());

		let a: u8 = pixel[0] >> 4;
		let r: u8 = pixel[0] & 0x0F;
		let g: u8 = pixel[1] >> 4;
		let b: u8 = pixel[1] & 0x0F;

		let r: u8 = ((u16::from(r) * 0xFF + 0x07) / 0x0F) as u8;
		let g: u8 = ((u16::from(g) * 0xFF + 0x07) / 0x0F) as u8;
		let b: u8 = ((u16::from(b) * 0xFF + 0x07) / 0x0F) as u8;
		let a: u8 = ((u16::from(a) * 0xFF + 0x07) / 0x0F) as u8;

		[r, g, b, a]
	}
}


impl std::fmt::Display for Argb4444Pixel {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		self.display(f)
	}
}
