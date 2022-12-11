use crate::PaaImage;
use crate::PaaResult;
use crate::PaaError::*;

use image::RgbaImage;


/// Wrapper around [`PaaImage`] that decodes mipmaps into [`image::RgbaImage`]
#[allow(missing_debug_implementations)]
#[derive(Clone)]
pub struct PaaDecoder {
	paa: PaaImage,
}


impl PaaDecoder {
	/// Create an instance of `Self` from a [`PaaImage`].
	pub fn with_paa(paa: PaaImage) -> Self {
		Self { paa }
	}


	/// Decode mipmap at [`PaaImage::mipmaps`]`[index]`.
	///
	/// # Errors
	/// - [`MipmapIndexOutOfRange`]: `index` is outside of bounds of [`PaaImage::mipmaps`].
	/// - other: [`PaaResult<PaaMipmap>`] at given index contains an error.
	///
	/// # Panics
	/// - If [`image::RgbaImage::from_vec`] fails.
	pub fn decode_nth(&self, index: usize) -> PaaResult<RgbaImage> {
		let mipmap = self.paa.mipmaps
			.get(index)
			.ok_or(MipmapIndexOutOfRange)?
			.as_ref()
			.map_err(Clone::clone)?;

		mipmap.decode()
	}


	/// Decode the first (largest) mipmap, see [`PaaDecoder::decode_nth`].
	///
	/// # Errors
	/// - [`MipmapIndexOutOfRange`]: `index` is outside of bounds of [`PaaImage::mipmaps`].
	/// - other: [`PaaResult<PaaMipmap>`] at given index contains an error.
	///
	/// # Panics
	/// - If [`image::RgbaImage::from_vec`] fails.
	pub fn decode_first(&self) -> PaaResult<RgbaImage> {
		self.decode_nth(0)
	}
}
