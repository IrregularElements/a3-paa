#![cfg_attr(doc, feature(doc_cfg))]
#![warn(missing_docs, unreachable_pub, clippy::all)]
#![allow(deprecated)]
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![warn(clippy::missing_errors_doc, clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]


#![doc = include_str!("../../README.md")]


mod macros;
mod mipmap;
mod pixel;
mod imageops;
mod cfgfile;
mod decode;
mod encode;

pub use mipmap::*;
pub use decode::*;
pub use encode::*;


use std::fmt::Debug;
use std::io::{Read, Seek, SeekFrom, Cursor};
use std::iter::Extend;
use std::default::Default;

#[cfg(feature = "arbitrary")] use arbitrary::{Arbitrary, Unstructured, Result as ArbitraryResult};
use bstr::BString;
use byteorder::{LittleEndian, ByteOrder, ReadBytesExt};
#[cfg(test)] use byteorder::BigEndian;
use deku::prelude::*;
use derive_more::{Display, Error};
use enum_utils::FromStr;
use image::{RgbaImage, Pixel};
use static_assertions::const_assert;
#[cfg(test)] use static_assertions::assert_impl_all;
use surety::Ensure;
use tap::prelude::*;
use bohemia_compression::*;

use PaaError::*;

/// [`std::result::Result`] parameterized with [`PaaError`]
pub type PaaResult<T> = Result<T, PaaError>;


/// `a3_paa`'s [`std::error::Error`]
#[derive(Debug, Display, Error, Clone)]
#[non_exhaustive]
pub enum PaaError {
	/// A function that reads from [`std::io::Read`] encountered early EOF.
	#[display(fmt = "Unexpected end of input file")]
	UnexpectedEof,

	/// Unexpected I/O error that is not UnexpectedEof.
	#[display(fmt = "Unexpected I/O error: {}", _0)]
	UnexpectedIoError(#[error(ignore)] std::io::ErrorKind),

	/// Unexpected integer conversion error.
	#[display(fmt = "Unexpected integer conversion error: {}", _0)]
	UnexpectedTryFromIntError(std::num::TryFromIntError),

	/// Attempted to read a PAA image with incorrect magic bytes.
	#[display(fmt = "Unknown PAA type: {:02x?}", _0)]
	UnknownPaaType(#[error(ignore)] [u8; 2]),

	/// Attempted to read a Tagg which does not start with the "GGAT" signature.
	#[display(fmt = "Attempted to read a TAGG which does not start with a \"GGAT\" signature")]
	UnexpectedTaggSignature,

	/// Attempted to read a Tagg with unknown name.
	#[display(fmt = "Attempted to read a TAGG with unexpected name: {:02x?}", _0)]
	UnknownTaggType(#[error(ignore)] [u8; 4]),

	/// Attempted to read a Tagg with unexpected indicated payload size.
	#[display(fmt = "Attempted to read a TAGG with unexpected indicated payload size")]
	UnexpectedTaggDataSize,

	/// Attempted to read a [`Tagg::Flag`] with unexpected transparency value.
	#[display(fmt = "Attempted to read a FLAGTAGG with unknown transparency value: {:02x?}", _0)]
	UnknownTransparencyValue(#[error(ignore)] u8),

	/// Attempted to read a [`Tagg::Swiz`] with unexpected swizzle value.
	#[display(fmt = "Attempted to read a SWIZTAGG with unknown swizzle values: {:02x?}", _0)]
	UnknownSwizzleValues(#[error(ignore)] [u8; 4]),

	/// Attempted to parse an unexpected swizzle value with FromStr.
	#[display(fmt = "Attempted to parse an unexpected swizzle value: {}", _0)]
	InvalidSwizzleString(#[error(ignore)] String),

	/// Attempted to parse a ChannelSwizzleId from a string that is not "A", "R", "G", or "B".
	#[display(fmt = "Attempted to parse an unexpected ChannelSwizzleId value: {}", _0)]
	InvalidChannelSwizzleIdString(#[error(ignore)] String),

	/// Attempted to construct or index a [`PaaPalette`] with number of colors
	/// overflowing a [`u16`][std::primitive::u16].
	#[display(fmt = "Attempted to construct or index a palette with number of colors overflowing a u16")]
	PaletteTooLarge,

	/// Mipmap returned by [`PaaMipmap::read_from`] or
	/// [`PaaMipmap::read_from_until_eof`] had zero width or zero height.
	#[display(fmt = "Read an empty mipmap")]
	EmptyMipmap,

	/// Mipmap start offset (as indicated in the file) is beyond EOF.
	#[display(fmt = "Mipmap start offset as indicated in metadata is beyond EOF")]
	MipmapOffsetBeyondEof,

	/// Some or all mipmap data (as indicated by mipmap data length) is beyond
	/// EOF.
	#[display(fmt = "Some or all mipmap data is beyond EOF")]
	MipmapDataBeyondEof,

	/// Input mipmap dimensions higher than 32768, or overflowing a length integer.
	#[display(fmt = "While encoding, received a mipmap with one or both dimensions larger than 32768, or overflowing a length integer")]
	MipmapTooLarge,

	/// Uncompressed mipmap data is not of the same size as computed by
	/// [`PaaType::predict_size`].  Enum members are width, height and
	/// [`predict_size`][PaaType::predict_size] result.
	#[error(ignore)]
	#[display(fmt = "Uncompressed mipmap data is not the same size as computed from dimensions (predict_size({}x{}) = {})", _0, _1, _2)]
	UnexpectedMipmapDataSize(u16, u16, usize),

	/// The [`PaaImage`] passed to [`PaaImage::to_bytes`] contained mipmap errors.
	#[display(fmt = "The PaaImage passed to PaaImage::to_bytes contained mipmap errors")]
	InputMipmapErrorWhileEncoding(usize, Box<PaaError>),

	/// [`PaaMipmap::to_bytes`] failed.
	#[display(fmt = "PaaMipmap::to_bytes failed")]
	MipmapErrorWhileSerializing(Box<PaaError>),

	/// A checked arithmetic operation triggered an unexpected under/overflow.
	#[display(fmt = "A checked arithmetic operation triggered an unexpected under/overflow")]
	ArithmeticOverflow,

	/// An error occurred while uncompressing RLE data (this likely means the
	/// data is incomplete).
	#[display(fmt = "An error occurred while uncompressing RLE data (compressed data likely truncated)")]
	RleError(BcError),

	/// DXT-LZO de/compression failed.
	#[display(fmt = "DXT-LZO decompression failed: {}", _0)]
	LzoError(/*MinilzoError*/ #[error(ignore)] String),

	/// LZSS decompression failed, uncompressed data is not of expected length.
	#[display(fmt = "LZSS decompression failed, uncompressed data is not of expected length")]
	LzssDecompressError,

	/// [`PaaMipmap::read_from`] was passed an LZSS-compressed [`PaaMipmap`]
	/// with incorrect additive checksum, or LZSS decompression resulted in
	/// incorrect data.
	#[display(fmt = "LZSS checksum present in mipmap differs from the checksum computed on uncompressed data")]
	LzssWrongChecksum,

	/// [`PaaDecoder::decode_nth`] received a mipmap index out of range.
	#[display(fmt = "Mipmap index out of range")]
	MipmapIndexOutOfRange,

	/// Generic parse error in TexConvert.cfg.
	#[display(fmt = "TexConvert parse error: {}", _0)]
	TexconvertParseError(nom::Err<String>),

	/// Attempted to parse a `TextureHints` class in TexConvert.cfg without a `name` field.
	#[display(fmt = "No name field in a TexConvert hint")]
	TexconvertNoName,

	/// Attempted to parse a `TextureHints` class in TexConvert.cfg with an invalid parent clause.
	#[display(fmt = "TexConvert hint attemps to inherit a non-existing parent: {}", _0)]
	TexconvertInvalidInherit(#[error(ignore)] String),

	/// Attempted to read an [`ArgbPixel`] from invalid data.
	#[doc(hidden)]
	#[display(fmt = "Attempted to read an ArgbPixel from invalid data")]
	PixelReadError,
}


impl From<std::io::Error> for PaaError {
	fn from(error: std::io::Error) -> Self {
		match error.kind() {
			std::io::ErrorKind::UnexpectedEof => UnexpectedEof,
			kind => UnexpectedIoError(kind),
		}
	}
}


impl From<std::num::TryFromIntError> for PaaError {
	fn from(error: std::num::TryFromIntError) -> Self {
		UnexpectedTryFromIntError(error)
	}
}


/// A single PAA texture file represented as a struct
#[derive(Default, Debug, Clone)]
pub struct PaaImage {
	/// Format of all mipmaps in the image.
	pub paatype: PaaType,
	/// PAA header metadata.
	pub taggs: Vec<Tagg>,
	/// RGB888 LUT for [`PaaType::IndexPalette`] mipmaps.
	pub palette: Option<PaaPalette>,
	/// PAA mipmaps.
	pub mipmaps: Vec<PaaResult<PaaMipmap>>,
}


impl PaaImage {
	/// Maximum number of mipmaps in a [`PaaImage`], as limited by
	/// [`Tagg::Offs`].
	pub const MAX_MIPMAPS: u8 = 15;


	/// Read a [`PaaImage`][Self] from an [`std::io::Read`].
	///
	/// # Errors
	/// - [`UnexpectedEof`]: Unexpected end of file.
	/// - [`UnexpectedIoError`]: Unexpected read error.
	/// - [`UnknownPaaType`]: If the input PAA does not have a correct magic sequence.
	/// - [`ArithmeticOverflow`]: If mipmap offsets overflow a [`u32`].
	/// - [`MipmapOffsetBeyondEof`]: PAA is truncated; EOF is in the middle of a mipmap.
	///
	/// # Panics
	/// - If backtracking [`std::io::Seek::seek()`] fails while parsing [`Tagg`]s.
	/// - If [`deku::DekuContainerWrite::to_bytes()`] fails.
	pub fn read_from<R: Read + Seek>(input: &mut R) -> PaaResult<Self> {
		// [TODO] Index palette support
		let paatype_bytes: [u8; 2] = input.read_exact_buffered(2)?
			.try_into()
			.expect("Could not convert paatype_bytes (this is a bug)");
		let (_, paatype) = PaaType::from_bytes((&paatype_bytes, 0))
			.map_err(|_| UnknownPaaType(paatype_bytes))?;

		let mut offsets = vec![0u32; 0];

		let (taggs, _) = Tagg::read_taggs_from(input)?;

		for t in taggs.iter() {
			if let Tagg::Offs { offsets: offs } = t {
				offsets = offs.clone();
			};
		};

		let palette = PaaPalette::read_from(input)?;

		if palette.is_some() {
			return Err(UnknownPaaType(PaaType::IndexPalette.to_bytes().unwrap().try_into().unwrap()));
		};

		let mipmaps = if offsets.is_empty() {
			PaaMipmap::read_from_until_eof(input, paatype)
		}
		else {
			PaaMipmap::read_from_with_offsets(input, &offsets, paatype)
		};

		let image = PaaImage { paatype, taggs, palette, mipmaps };

		Ok(image)
	}


	/// Wrap `input` with a [`Cursor`][std::io::Cursor] and
	/// [`read_from`][`Self::read_from`] from it.
	///
	/// # Errors
	/// - [`UnexpectedEof`]: Unexpected end of file.
	/// - [`UnexpectedIoError`]: Unexpected read error.
	/// - [`UnknownPaaType`]: If the input PAA does not have a correct magic sequence.
	/// - [`ArithmeticOverflow`]: If mipmap offsets overflow a [`u32`].
	/// - [`MipmapOffsetBeyondEof`]: PAA is truncated; EOF is in the middle of a mipmap.
	///
	/// # Panics
	/// - If backtracking [`std::io::Seek::seek()`] fails while parsing [`Tagg`]s.
	/// - If [`deku::DekuContainerWrite::to_bytes()`] fails.
	pub fn from_bytes(input: &[u8]) -> PaaResult<Self> {
		let mut cursor = Cursor::new(input);
		Self::read_from(&mut cursor)
	}


	/// Convert self to PAA data as `Vec<u8>`.
	///
	/// Ignores input `Tagg::Offs` and regenerates offsets based on actual mipmap
	/// data.
	///
	/// # Errors
	/// - [`ArithmeticOverflow`]: [`Tagg`]s and [`PaaPalette`] overflow a [`u32`].
	/// - [`InputMipmapErrorWhileEncoding`]: One of [`PaaImage::mipmaps`] contained an error.
	/// - [`MipmapErrorWhileSerializing`]: [`PaaMipmap::to_bytes()`] returned an error.
	/// - [`PaletteTooLarge`]: [`PaaPalette`] pixel count overflows a [`u16`].
	///
	/// # Panics
	/// - If mipmap offsets overflow a [`u32`].  This may only happen with a lot of
	///   [`Tagg`]s and large mipmaps.
	/// - If [`deku::DekuContainerWrite::to_bytes()`] fails.
	pub fn to_bytes(&self) -> PaaResult<Vec<u8>> {
		let mut buf: Vec<u8> = Vec::with_capacity(10_000_000);

		buf.extend(self.paatype.to_bytes().unwrap());

		for t in &self.taggs {
			if let Tagg::Offs { .. } = t {
				continue;
			};

			buf.extend(t.to_bytes());
		};

		#[allow(clippy::cast_possible_truncation)]
		let offs_length = Tagg::Offs { offsets: vec![] }.to_bytes().len() as u32;

		let palette_data =
			if let Some(p) = &self.palette {
				p.to_bytes()?
			}
			else {
				vec![0u8, 0]
			};

		let mipmaps_offset = {
			let buf_len = buf.len().checked();
			let palette_len = palette_data.len().checked();
			// [SAFETY]: usize is implicitly guaranteed to be at least 16 bits
			// wide, and the length of an empty OFFSTAGG fits in that.
			buf_len + (offs_length as usize) + palette_len
		};

		let mipmap_blocks = self.mipmaps
			.iter()
			.enumerate()
			.map(|(i, m)| {
				let m = m.clone().map_err(|e| InputMipmapErrorWhileEncoding(i, Box::new(e)))?;
				m.to_bytes().map_err(|e| MipmapErrorWhileSerializing(Box::new(e)))
			})
			.collect::<PaaResult<Vec<Vec<u8>>>>()?;

		let mipmap_block_offsets: Vec<u32> = mipmap_blocks
			.iter()
			.scan(0usize.checked(), |acc, b| {
				let current = *acc;
				let offset = current + mipmaps_offset;
				*acc += b.len().checked();
				Some(offset)
			})
			.map(|c| c.ok_or(ArithmeticOverflow))
			.collect::<PaaResult<Vec<usize>>>()?
			.iter()
			.map(|c| <usize as TryInto<u32>>::try_into(*c).map_err(|_| ArithmeticOverflow))
			.collect::<PaaResult<Vec<u32>>>()?;

		let new_offs = Tagg::Offs { offsets: mipmap_block_offsets };
		buf.extend(new_offs.to_bytes());

		buf.extend(palette_data);

		for m in mipmap_blocks {
			buf.extend(m);
		};

		buf.extend([0u8; 6]);

		Ok(buf)
	}
}


/// Bitmap encoding used by all [mipmaps][`PaaImage::mipmaps`] of a given PAA
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromStr, DekuRead, DekuWrite)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
#[enumeration(case_insensitive)]
#[deku(type = "u16", endian = "little")]
pub enum PaaType {
	// See `int __stdcall sub_4276E0(void *Block, int)` (ImageToPAA v1.0.0.3).

	/// 1 byte (offset into the index palette, which contains BGR 8:8:8).
	#[deprecated = "[TODO] Index palette format is not implemented"]
	#[deku(id = "0x47_47")]
	IndexPalette,

	/// 8 bits alpha, 8 bits grayscale.
	#[deku(id = "0x80_80")]
	Ai88,

	/// RGBA 5:5:5:1 in a little-endian 2-byte integer.
	#[deku(id = "0x15_55")]
	Argb1555,

	/// ARGB 4:4:4:4 in a little-endian 2-byte integer.
	#[deku(id = "0x44_44")]
	Argb4444,

	/// RGBA 8:8:8:8.
	#[deku(id = "0x88_88")]
	Argb8888,

	/// `[TODO]`
	#[deku(id = "0xFF_01")]
	Dxt1,

	/// `[TODO]`
	#[deprecated]
	#[deku(id = "0xFF_02")]
	Dxt2,

	/// `[TODO]`
	#[deprecated]
	#[deku(id = "0xFF_03")]
	Dxt3,

	/// `[TODO]`
	#[deprecated]
	#[deku(id = "0xFF_04")]
	Dxt4,

	/// DXT5 (BC3) texture.
	#[deku(id = "0xFF_05")]
	Dxt5,
}


impl Default for PaaType {
	/// Returns [`Dxt5`][`PaaType::Dxt5`].
	fn default() -> Self {
		PaaType::Dxt5
	}
}


impl PaaType {
	/// Calculate the size in bytes of uncompressed mipmap data from its width
	/// and height in pixels.
	pub const fn predict_size(&self, width: u16, height: u16) -> usize {
		use PaaType::*;

		const_assert!(std::mem::size_of::<usize>() >= 4);

		let mut result = width as usize * height as usize;

		match self {
			Dxt1 => { result /= 2 },
			IndexPalette | Dxt2 | Dxt3 | Dxt4 | Dxt5 => (),
			Argb4444 | Argb1555 | Ai88 => { result *= 2 },
			Argb8888 => { result *= 4 },
		};

		result
	}


	/// Return true if the [`PaaType`] is DXTn.
	///
	/// # Example
	/// ```
	/// # use a3_paa::PaaType;
	/// assert!(PaaType::Dxt5.is_dxtn());
	/// assert!(!PaaType::Argb1555.is_dxtn());
	/// ```
	pub const fn is_dxtn(&self) -> bool {
		use PaaType::*;
		matches!(self, Dxt1 | Dxt2 | Dxt3 | Dxt4 | Dxt5)
	}


	/// Return true if the [`PaaType`] is ARGBxxxx.
	///
	/// # Example
	/// ```
	/// # use a3_paa::PaaType;
	/// assert!(PaaType::Argb1555.is_argb());
	/// assert!(!PaaType::Dxt5.is_argb());
	/// assert!(!PaaType::Ai88.is_argb());
	/// ```
	pub const fn is_argb(&self) -> bool {
		use PaaType::*;
		matches!(self, Argb1555 | Argb4444 | Argb8888)
	}


	/// Return true if this PAA type contains the alpha channel (`true` for
	/// all types except [`IndexPalette`][`PaaType::IndexPalette`]).
	///
	/// # Example
	/// ```
	/// # use a3_paa::PaaType;
	/// assert!(PaaType::Dxt5.has_alpha());
	/// assert!(!PaaType::IndexPalette.has_alpha());
	/// ```
	pub const fn has_alpha(&self) -> bool {
		use PaaType::*;
		!matches!(self, IndexPalette)
	}
}


/// Metadata frame present in PAA headers
#[derive(Debug, Display, Clone, PartialEq, Eq)]
pub enum Tagg {
	/// Average color value.
	#[display(fmt = "Avgc {{ {} }}", rgba)]
	Avgc {
		/// `[TODO]`
		rgba: Bgra8888Pixel,
	},

	/// Maximum color value.
	#[display(fmt = "Maxc {{ {} }}", rgba)]
	Maxc {
		/// `[TODO]`
		rgba: Bgra8888Pixel,
	},

	/// PAA flags (only transparency/alpha interpolation is currently
	/// documented).
	#[display(fmt = "Flag {{ {} }}", transparency)]
	Flag {
		/// Texture transparency type.
		transparency: Transparency
	},

	/// Texture swizzle (subpixel mapping) algorithm.
	#[display(fmt = "Swiz {{ {} }}", swizzle)]
	Swiz {
		/// Specific mapping that was used to encode the PAA.
		swizzle: ArgbSwizzle,
	},

	/// Procedural texture code.
	#[display(fmt = "{:?}", self)]
	Proc {
		/// `[TODO]`
		code: TextureMacro,
	},

	/// Mipmap offsets.
	#[display(fmt = "{:X?}", self)]
	Offs {
		/// Offsets into the file for each respective mipmap.
		offsets: Vec<u32>
	},
}


impl Tagg {
	/// Serialize a Tagg into PAA-ready data.
	///
	/// # Panics
	/// - If [`deku::DekuContainerWrite::to_bytes()`] fails.
	pub fn to_bytes(&self) -> Vec<u8> {
		#[allow(clippy::cast_possible_truncation)]
		const U32_SIZE: u32 = std::mem::size_of::<u32>() as u32;

		let mut bytes: Vec<u8> = Vec::with_capacity(256);
		bytes.extend("GGAT".as_bytes());
		bytes.extend(self.as_taggname().as_bytes());

		match self {
			Self::Avgc { rgba } => {
				bytes.extend_with_uint::<LittleEndian, _, 4>(U32_SIZE);
				bytes.extend(rgba.to_bytes().unwrap());
			},

			Self::Maxc { rgba } => {
				bytes.extend_with_uint::<LittleEndian, _, 4>(U32_SIZE);
				bytes.extend(rgba.to_bytes().unwrap());
			},

			Self::Flag { transparency } => {
				bytes.extend_with_uint::<LittleEndian, _, 4>(U32_SIZE);
				bytes.extend(transparency.to_bytes().unwrap());
				bytes.extend([0x00u8, 0, 0]);
			},

			Self::Swiz { swizzle } => {
				bytes.extend_with_uint::<LittleEndian, _, 4>(U32_SIZE);
				bytes.extend(swizzle.to_bytes().unwrap());
			},

			Self::Proc { code } => {
				// Tagg data length is guaranteed to fit in a u32
				#[allow(clippy::cast_possible_truncation)]
				let len = (code.text[..]).len() as u32;
				bytes.extend_with_uint::<LittleEndian, _, 4>(len);
				bytes.extend(&code.text[..]);
			},

			Self::Offs { offsets } => {
				#[allow(clippy::cast_possible_truncation)]
				let len = (16 * std::mem::size_of::<u32>()) as u32;
				bytes.extend_with_uint::<LittleEndian, _, 4>(len);

				let mut buf = [0u8; 16*4];
				let mut offsets = offsets.clone();
				if offsets.len() != 16 {
					offsets.resize(16, 0);
				};

				LittleEndian::write_u32_into(&offsets[..], &mut buf);
				bytes.extend(&buf);
			},
		};

		bytes
	}


	/// Validate Tagg metadata contained in `data`: "TAGG" signature, tag name,
	/// and payload length.  Returns `PaaResult<(name: String, payload_size: u32)>`.
	///
	/// # Errors
	/// - [`UnexpectedTaggSignature`]: TAGG data does not start with "GGAT".
	/// - [`UnknownTaggType`]: TAGG signature is not [`Tagg::is_valid_taggname`].
	///
	/// # Panics
	/// - If [`String::as_bytes()`] fails (should never happen).
	/// - If &[u8] of length 4 fails to convert to [u8; 4] (should never happen).
	///
	/// # Example
	/// ```
	/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
	/// # use a3_paa::Tagg;
	/// let offsdata = Tagg::Offs { offsets: vec![] }.to_bytes();
	/// let headdata = (&offsdata[..12]).try_into()?;
	/// let (taggname, payload_size) = Tagg::try_head_from(headdata)?;
	/// assert_eq!(taggname, "SFFO");
	/// assert_eq!(payload_size as usize, (&offsdata[12..]).len()); // 64 for a well-formed OFFSTAGG
	/// # Ok(()) }
	/// ```
	pub fn try_head_from(data: &[u8; 12]) -> PaaResult<(String, u32)> {
		let taggsig = &data[0..4];

		// "GGAT" signature
		if taggsig != [0x47u8, 0x47, 0x41, 0x54] {
			return Err(UnexpectedTaggSignature);
		};

		let taggname = &data[4..8];
		let taggname: String = std::str::from_utf8(taggname)
			.map_err(|_| UnknownTaggType((taggname).try_into().unwrap()))?
			.into();

		if !Self::is_valid_taggname(&taggname) {
			return Err(UnknownTaggType(taggname.as_bytes().try_into().unwrap()));
		};

		let payload_length = LittleEndian::read_u32(&data[8..12]);

		Ok((taggname, payload_length))
	}


	/// Construct a [`Tagg`] from its name (e.g. "OFFS") and payload.
	///
	/// # Errors
	/// - [`UnexpectedTaggSignature`]: Encountered an unknown type of [`Tagg`].
	/// - [`UnexpectedTaggDataSize`]: Payload was of an unexpected length.
	///
	/// # Panics
	/// - If [`deku::DekuContainerRead::from_bytes`] fails (should never happen).
	/// - If &[u8] of length 4 fails to convert to [u8; 4] (should never happen).
	pub fn from_name_and_payload(taggname: &str, data: &[u8]) -> PaaResult<Self> {
		if taggname.len() != 4 {
			return Err(UnexpectedTaggSignature);
		};

		match taggname {
			"CGVA" => {
				if data.len() != 4 {
					return Err(UnexpectedTaggDataSize);
				};
				let (_, rgba) = Bgra8888Pixel::from_bytes((data, 0)).unwrap();
				Ok(Self::Avgc { rgba })
			},

			"CXAM" => {
				if data.len() != 4 {
					return Err(UnexpectedTaggDataSize);
				};
				let (_, rgba) = Bgra8888Pixel::from_bytes((data, 0)).unwrap();
				Ok(Self::Maxc { rgba })
			},

			"GALF" => {
				if data.len() != 4 {
					return Err(UnexpectedTaggDataSize);
				};
				let (_, transparency) = Transparency::from_bytes((&data[0..1], 0))
					.map_err(|_| UnknownTransparencyValue(data[0]))?;
				Ok(Self::Flag { transparency })
			},

			"ZIWS" => {
				if data.len() != 4 {
					return Err(UnexpectedTaggDataSize);
				};
				let (_, swizzle) = ArgbSwizzle::from_bytes((data, 0))
					.map_err(|_| UnknownSwizzleValues(data[0..4].try_into().unwrap()))?;
				Ok(Self::Swiz { swizzle })
			},

			"CORP" => {
				let text = BString::from(data);
				Ok(Self::Proc { code: TextureMacro { text } })
			},

			"SFFO" => {
				// [NOTE] Offset vectors that are not of length 16 do not
				// apparently occur; however, we do allow them nonetheless
				if data.len() % std::mem::size_of::<u32>() != 0 {
					return Err(UnexpectedTaggDataSize);
				};

				let offset_count = data.len() / std::mem::size_of::<u32>();
				let mut offsets = vec![0u32; offset_count];

				LittleEndian::read_u32_into(data, &mut offsets[..]);

				if let Some(idx) = offsets.iter().position(|x| *x == 0) {
					offsets.truncate(idx);
				};

				Ok(Self::Offs { offsets })
			},

			_ => Err(UnknownTaggType(taggname.as_bytes().try_into().unwrap())),
		}
	}


	/// Try to read a [`Tagg`] from [`Read`][std::io::Read].  If the read fails,
	/// this function attempts to seek back to the starting point.
	///
	/// # Errors
	/// - [`UnexpectedEof`]:
	/// - [`UnexpectedIoError`]:
	/// - [`UnexpectedTryFromIntError`]:
	/// - [`UnknownTaggType`]: Encountered an unknown type of [`Tagg`].
	/// - [`UnexpectedTaggSignature`]: No "TAGG" signature at the beginning.
	/// - [`UnexpectedTaggDataSize`]: Payload was of an unexpected length.
	///
	/// # Panics
	/// - If the backtracking seek fails after an error occurs.
	pub fn read_tagg_from<R: Read + Seek>(input: &mut R) -> PaaResult<Self> {
		let start_position = input.stream_position()?;

		let get_tagg = |input: &mut R| -> PaaResult<Self> {
			let mut tagghead_data = [0u8; 12];
			input.read_exact(&mut tagghead_data)?;
			let (taggname, payload_length) = Tagg::try_head_from(&tagghead_data)?;
			let payload = input.read_exact_buffered(payload_length.try_into()?)?;
			let tagg = Tagg::from_name_and_payload(&taggname, &payload)?;
			Ok(tagg)
		};

		let tagg = get_tagg(input)
			.tap_err(|_| { let _ = input.seek(SeekFrom::Start(start_position)).expect("Backtracking seek failed"); })?;

		Ok(tagg)
	}


	/// Read as many [`Tagg`]s as possible from a [`Read`][std::io::Read].
	/// This function returns a tuple of (1) the vector of read [`Tagg`]s, and
	/// (2) the error that interrupted reading.  When reading a well-formed PAA
	/// file, (2) is going to be [`UnknownTaggType`] or
	/// [`UnexpectedTaggSignature`].
	///
	/// # Errors
	/// - [`UnexpectedIoError`]: If [`Seek::stream_position()`] fails.
	///
	/// # Panics
	/// - If the backtracking seek fails after an error occurs.
	pub fn read_taggs_from<R: Read + Seek>(input: &mut R) -> PaaResult<(Vec<Self>, PaaError)> {
		let mut result: Vec<Self> = Vec::with_capacity(10);
		let error: PaaError;

		loop {
			let tagg = Tagg::read_tagg_from(input);

			match tagg {
				Ok(t) => result.push(t),
				Err(e) => { error = e; break; },
			};
		};

		Ok((result, error))
	}


	/// Return the 4-byte signature (as ASCII String), e.g. "SFFO" for the
	/// offsets Tagg.
	pub fn as_taggname(&self) -> &'static str {
		match self {
			Self::Avgc { .. } => "CGVA",
			Self::Maxc { .. } => "CXAM",
			Self::Flag { .. } => "GALF",
			Self::Swiz { .. } => "ZIWS",
			Self::Proc { .. } => "CORP",
			Self::Offs { .. } => "SFFO",
		}
	}


	/// Check if `name` is a valid 4-character Tagg name as represented in the
	/// file (e.g. "SFFO").
	///
	/// # Example
	/// ```
	/// # use a3_paa::Tagg;
	/// assert!(Tagg::is_valid_taggname(&Tagg::Maxc { rgba: Default::default() }.as_taggname()));
	/// ```
	pub fn is_valid_taggname(name: &str) -> bool {
		matches!(name, "CGVA" | "CXAM" | "GALF" | "ZIWS" | "CORP" | "SFFO")
	}
}


#[cfg(feature = "arbitrary")]
impl<'a> Arbitrary<'a> for Tagg {
	fn arbitrary(input: &mut Unstructured) -> ArbitraryResult<Self> {
		use Tagg::*;

		let variant: usize = input.int_in_range(1..=6)?;

		let result = match variant {
			1 => Avgc { rgba: input.arbitrary()? },

			2 => Maxc { rgba: input.arbitrary()? },

			3 => Flag { transparency: input.arbitrary()? },

			4 => Swiz { swizzle: input.arbitrary()? },

			5 => Proc { code: input.arbitrary()? },

			6 => {
				let offs_len: usize = input.int_in_range(0..=16)?;
				let mut offsets: Vec<u32> = vec![0u32; offs_len];

				for o in &mut offsets {
					*o = input.arbitrary()?;
				};

				if let Some(idx) = offsets.iter().position(|x| *x == 0) {
					offsets.truncate(idx);
				};

				Offs { offsets }
			},

			_ => unreachable!(),
		};

		Ok(result)
	}
}


/// Lookup table for [`PaaType::IndexPalette`] PAAs containing [`Bgr888Pixel`]
/// data
///
/// NOTE: The binary layout of this palette limits the count of contained
/// pixels to [`u16::MAX`]; however, [`PaaType::IndexPalette`] can only index
/// [`u8::MAX`].
#[derive(Default, Debug, Clone)]
pub struct PaaPalette {
	pixels: Vec<Bgr888Pixel>,
}


impl PaaPalette {
	/// Construct an instance of [`Self`] from a slice of [pixels][`Bgr888Pixel`].
	///
	/// # Errors
	/// - [`PaletteTooLarge`]: `pixels.len()` overflows a [`u16`].
	pub fn with_pixels(pixels: &[Bgr888Pixel]) -> PaaResult<Self> {
		if pixels.len() > u16::MAX.into() {
			return Err(PaletteTooLarge);
		};

		let result = Self { pixels: pixels.to_vec() };

		Ok(result)
	}


	/// Return the pixel at `index`.
	///
	/// NOTE: [`PaaType::IndexPalette`] can only index up to [`u8::MAX`].
	///
	/// # Errors
	/// - [`PaletteTooLarge`]: `index` is out of bounds.
	pub fn get(&self, index: u16) -> PaaResult<&Bgr888Pixel> {
		self.pixels.get(<u16 as Into<usize>>::into(index)).ok_or(PaletteTooLarge)
	}


	/// Convert self to PAA data.
	///
	/// # Errors
	/// - [`PaletteTooLarge`]: [`self.pixels.len()`] overflows [`u16`].
	///
	/// # Panics
	/// - [`DekuContainerWrite::to_bytes`] fails (should never happen).
	pub fn to_bytes(&self) -> PaaResult<Vec<u8>> {
		const_assert!(std::mem::size_of::<usize>() >= std::mem::size_of::<u16>());

		if self.pixels.len() > u16::MAX as usize {
			return Err(PaletteTooLarge);
		};

		#[allow(clippy::cast_possible_truncation)]
		let ntriplets = self.pixels.len() as u16;
		let capacity: usize = {
			let mut c = <u16 as Into<usize>>::into(ntriplets).checked();
			c *= 3;
			c += 2;
			c.ok_or(ArithmeticOverflow)?
		};
		let mut buf: Vec<u8> = Vec::with_capacity(capacity);

		buf.extend_with_uint::<LittleEndian, _, 2>(ntriplets);

		for pixel in &self.pixels {
			buf.extend(pixel.to_bytes().unwrap());
		};

		Ok(buf)
	}


	/// Return `Ok(None)` if palette is empty, `Ok(palette)` otherwise.
	///
	/// # Errors
	/// - [`UnexpectedEof`]: Encountered EOF before reading the entire palette.
	/// - [`UnexpectedIoError`]: Encountered an I/O error before reading the
	///   entire palette.
	///
	/// # Panics
	/// - Could not convert a &[u8] of length 3 to [u8; 3] (should never happen).
	/// - [`DekuContainerWrite::to_bytes`] fails (should never happen).
	pub fn read_from<R: Read>(input: &mut R) -> PaaResult<Option<Self>> {
		const_assert!(std::mem::size_of::<usize>() >= std::mem::size_of::<u16>());

		let count = input.read_u16::<LittleEndian>()?;
		#[allow(clippy::cast_possible_truncation)]
		let mut pixels: Vec<Bgr888Pixel> = Vec::with_capacity(count as usize);

		if count == 0 {
			return Ok(None);
		};

		for i in 0..count {
			let buf: [u8; 3] = input.read_exact_buffered(3)?.try_into().expect("Could not convert buf (this is a bug)");
			let (_, pixel) = Bgr888Pixel::from_bytes((&buf, 0)).unwrap();
			#[allow(clippy::cast_possible_truncation)]
			pixels.insert(i as usize, pixel);
		};

		Ok(Some(Self { pixels }))
	}
}


/// BGR888 pixel used in [`PaaPalette`]
#[derive(Default, Debug, Clone, Copy, DekuRead, DekuWrite)]
pub struct Bgr888Pixel {
	#[allow(missing_docs)]
	pub b: u8,
	#[allow(missing_docs)]
	pub g: u8,
	#[allow(missing_docs)]
	pub r: u8,
}




/// The color data used in AVGCTAGG and MAXCTAGG; its byte layout is B:G:R:A
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
pub struct Bgra8888Pixel {
	#[allow(missing_docs)]
	pub b: u8,
	#[allow(missing_docs)]
	pub g: u8,
	#[allow(missing_docs)]
	pub r: u8,
	#[allow(missing_docs)]
	pub a: u8,
}


impl std::fmt::Display for Bgra8888Pixel {
	#[allow(clippy::cast_lossless)]
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(f, "<r={:.3}> <g={:.3}> <b={:.3}> <a={:.3}>",
			self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0, self.a as f32 / 255.0)
	}
}


impl From<image::Rgba<u8>> for Bgra8888Pixel {
	fn from(rgba: image::Rgba<u8>) -> Self {
		let b = rgba.0[2];
		let g = rgba.0[1];
		let r = rgba.0[0];
		let a = rgba.0[3];
		Self { b, g, r, a }
	}
}


/// Alpha interpolation algorithm used when the texture is rendered
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
#[deku(type = "u8")]
pub enum Transparency {
	/// Transparency disabled
	#[display(fmt = "<no transparency>")]
	#[deku(id = "0x00")]
	None,

	/// Transparency enabled, alpha channel interpolation enabled
	#[display(fmt = "<transparent, interpolated alpha>")]
	#[deku(id = "0x01")]
	AlphaInterpolated,

	/// Transparency enabled, alpha channel interpolation disabled
	#[display(fmt = "<transparent, non-interpolated alpha>")]
	#[deku(id = "0x02")]
	AlphaNotInterpolated,
}


impl Default for Transparency {
	fn default() -> Self {
		Transparency::AlphaInterpolated
	}
}


/// PAA texture ARGB swizzle data (see [`ChannelSwizzle`])
#[derive(Debug, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite)]
pub struct ArgbSwizzle {
	#[allow(missing_docs)]
	#[deku(ctx = "ChannelSwizzleId::Alpha")]
	pub a: ChannelSwizzle,
	#[allow(missing_docs)]
	#[deku(ctx = "ChannelSwizzleId::Red")]
	pub r: ChannelSwizzle,
	#[allow(missing_docs)]
	#[deku(ctx = "ChannelSwizzleId::Green")]
	pub g: ChannelSwizzle,
	#[allow(missing_docs)]
	#[deku(ctx = "ChannelSwizzleId::Blue")]
	pub b: ChannelSwizzle,
}


impl Default for ArgbSwizzle {
	fn default() -> Self {
		Self::new()
	}
}


impl std::fmt::Display for ArgbSwizzle {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		match self {
			s if s.is_noop() => write!(f, "(no-op)"),
			_ => write!(f, "{}, {}, {}, {}", self.a, self.r, self.g, self.b),
		}
	}
}


#[cfg(feature = "arbitrary")]
impl<'a> Arbitrary<'a> for ArgbSwizzle {
	fn arbitrary(input: &mut Unstructured) -> ArbitraryResult<Self> {
		let a: ChannelSwizzle = ChannelSwizzle { target: ChannelSwizzleId::Alpha, ..input.arbitrary()? };
		let r: ChannelSwizzle = ChannelSwizzle { target: ChannelSwizzleId::Red, ..input.arbitrary()? };
		let g: ChannelSwizzle = ChannelSwizzle { target: ChannelSwizzleId::Green, ..input.arbitrary()? };
		let b: ChannelSwizzle = ChannelSwizzle { target: ChannelSwizzleId::Blue, ..input.arbitrary()? };
		Ok(ArgbSwizzle { a, r, g, b})
	}
}


impl ArgbSwizzle {
	/// Create a new ArgbSwizzle with no-op values (mapping alpha to alpha, etc).
	///
	/// # Example
	/// ```
	/// # use a3_paa::*;
	/// let pix_i = [0x11u8, 0x22, 0x33, 0x44];
	/// let pix_o = ArgbSwizzle::new().to_rgba8_map()(&pix_i);
	/// assert_eq!(pix_i, pix_o);
	/// ```
	pub const fn new() -> Self {
		ArgbSwizzle {
			a: ChannelSwizzle::with_target(ChannelSwizzleId::Alpha),
			r: ChannelSwizzle::with_target(ChannelSwizzleId::Red),
			g: ChannelSwizzle::with_target(ChannelSwizzleId::Green),
			b: ChannelSwizzle::with_target(ChannelSwizzleId::Blue),
		}
	}


	/// Parse ARGB swizzle values from respective A, R, G and B strings (in the
	/// same format as specified in `TexConvert.cfg`).
	///
	/// # Errors
	/// - [`InvalidSwizzleString`]: Some of the input strings were invalid.
	///
	/// # Example
	/// ```
	/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
	/// # use a3_paa::{ArgbSwizzle, ChannelSwizzleId::*, ChannelSwizzleData::*, ChannelSwizzleFill::*};
	/// let swiz = ArgbSwizzle::parse_argb("A", "R", "1", "1-B")?;
	/// assert!(matches!(swiz.g.data, Fill { value: FillFF }));
	/// # Ok(()) }
	/// ```
	pub fn parse_argb(a: &str, r: &str, g: &str, b: &str) -> PaaResult<Self> {
		let a = ChannelSwizzle::parse_data_with_target(a, ChannelSwizzleId::Alpha)?;
		let r = ChannelSwizzle::parse_data_with_target(r, ChannelSwizzleId::Red)?;
		let g = ChannelSwizzle::parse_data_with_target(g, ChannelSwizzleId::Green)?;
		let b = ChannelSwizzle::parse_data_with_target(b, ChannelSwizzleId::Blue)?;
		let result = ArgbSwizzle { a, r, g, b };

		Ok(result)
	}


	/// Return an [`FnMut`] that acts on an RGBA8888 pixel, processing it according
	/// to the value of `self`.  See also [`ChannelSwizzle::to_subpixel_map()`].
	pub fn to_rgba8_map(&self) -> Box<dyn FnMut(&[u8; 4]) -> [u8; 4]> {
		let mut a_flt = self.a.to_subpixel_map();
		let mut r_flt = self.r.to_subpixel_map();
		let mut g_flt = self.g.to_subpixel_map();
		let mut b_flt = self.b.to_subpixel_map();

		let lambda = move |src: &[u8; 4]| -> [u8; 4] {
			let mut dst = *src;
			a_flt(src, &mut dst);
			r_flt(src, &mut dst);
			g_flt(src, &mut dst);
			b_flt(src, &mut dst);
			dst
		};

		Box::new(lambda)
	}


	/// Apply the swizzle algorithm to every pixel in `image`.
	///
	/// # Panics
	/// - If `&[image::Subpixel]` fails to convert to `[u8; 4]`.
	///
	/// # Example
	/// ```no_run
	/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
	/// # use a3_paa::ArgbSwizzle; use image::RgbaImage;
	/// // The swizzle for class sky { .. } from TexConvert.cfg
	/// let swiz = ArgbSwizzle::parse_argb("1-G", "R", "1-A", "B")?;
	/// let mut image = image::open("sky_clear_sky.png")?.into_rgba8();
	/// swiz.apply_to_image(&mut image);
	/// # Ok(()) }
	/// ```
	pub fn apply_to_image(&self, image: &mut RgbaImage) {
		let mut map = self.to_rgba8_map();

		for pixel in image.pixels_mut() {
			let src = pixel.channels();
			let dst = map(src.try_into().unwrap());
			pixel.channels_mut().copy_from_slice(&dst);
		};
	}


	/// Returns `true` if `self` maps every channel to itself, i.e., if the
	/// swizzle does not change any channel.
	pub fn is_noop(&self) -> bool {
		self.a.is_noop() && self.r.is_noop() && self.g.is_noop() && self.b.is_noop()
	}
}


/// Swizzle information for a single ARGB channel
///
/// Some PAA textures apply "swizzle" to its channels during conversion to PAA.
/// The specific swizzle algorithm is described by the `TexConvert.cfg` file
/// from TexView (see also: [`TextureHints`]) and depends on the texture class
/// (as determined by its file name suffix).  Here's an example of a swizzle
/// definition from that file:
///
/// ```text
/// class normalmap_vhq {
///   name = "*_novhq.*";
///   <..>
///   channelSwizzleA = "1-R";
///   channelSwizzleR = "1";
///   channelSwizzleG = "G";
///   channelSwizzleB = "1";
///   <..>
/// };
/// ```
///
/// In this case, the swizzle values mean that, e.g., the PAA alpha channel is
/// computed from the original image's negated red channel value, the PAA red
/// channel is filled with all ones, etc.
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
#[deku(ctx = "tgt: ChannelSwizzleId")]
#[display(fmt = "<{}={}>", target, data)]
pub struct ChannelSwizzle {
	/// PAA channel being written into.
	#[deku(skip, default = "tgt")]
	pub target: ChannelSwizzleId,
	/// Data that's being written.
	#[deku(pad_bits_before = "4")]
	pub data: ChannelSwizzleData,
}


impl ChannelSwizzle {
	/// Create a no-op [`ChannelSwizzle`] that targets a specific channel.
	pub const fn with_target(target: ChannelSwizzleId) -> Self {
		ChannelSwizzle {
			target,
			data: ChannelSwizzleData::Source {
				neg_flag: false,
				source: target,
			},
		}
	}


	/// Parse a channel swizzle operation from a `&str`, and construct a
	/// [`ChannelSwizzle`] from the operation and the target channel.
	///
	/// # Errors
	/// - [`InvalidSwizzleString`]: If failed to parse `data`.
	///
	/// # Example
	/// ```
	/// # use a3_paa::{ChannelSwizzle, ChannelSwizzleId, ChannelSwizzleData};
	/// let swiz_alpha = ChannelSwizzle::parse_data_with_target("1-G", ChannelSwizzleId::Alpha).unwrap();
	/// assert_eq!(swiz_alpha.target, ChannelSwizzleId::Alpha);
	/// assert_eq!(swiz_alpha.data, ChannelSwizzleData::Source { neg_flag: true, source: ChannelSwizzleId::Green });
	/// ```
	pub fn parse_data_with_target(data: &str, target: ChannelSwizzleId) -> PaaResult<Self> {
		let data = data.parse::<ChannelSwizzleData>()?;
		let result = ChannelSwizzle { target, data };
		Ok(result)
	}


	/// Return a function object that acts on two RGBA8888 pixels (source and
	/// destination, respectively; each represented as `[u8; 4]`), applying
	/// swizzle to a single channel.
	///
	/// # Example
	/// ```
	/// # use a3_paa::{ChannelSwizzle, ChannelSwizzleId};
	/// let pixel_in = [0x00u8, 0x00, 0x00, 0x00];
	/// let mut pixel_out = pixel_in;
	/// ChannelSwizzle::parse_data_with_target("1", ChannelSwizzleId::Green)
	///     .unwrap()
	///     .to_subpixel_map()(&pixel_in, &mut pixel_out);
	/// assert_eq!(pixel_out[ChannelSwizzleId::Green as usize], 0xFF);
	/// ```
	pub fn to_subpixel_map(&self) -> Box<dyn FnMut(&[u8; 4], &mut [u8; 4])> {
		use ChannelSwizzleData::*;

		let target_idx = self.target as usize;

		match self.data {
			Source { neg_flag: false, source } => {
				let source_idx = source as usize;
				Box::new(move |src: &[u8; 4], dst: &mut [u8; 4]| { dst[target_idx] = src[source_idx] })
			},

			Source { neg_flag: true, source } => {
				let source_idx = source as usize;
				Box::new(move |src: &[u8; 4], dst: &mut [u8; 4]| { dst[target_idx] = 0xFF - src[source_idx] })
			},

			Fill { value } => {
				let fill_byte: u8 = value as u8;
				Box::new(move |_: &[u8; 4], dst: &mut [u8; 4]| { dst[target_idx] = fill_byte })
			},
		}
	}


	/// Returns `true` if `self` maps [`Self::target`] to itself.
	///
	/// # Example
	/// ```
	/// # use a3_paa::*;
	/// use a3_paa::ChannelSwizzleId::*;
	/// let data = ChannelSwizzleData::Source { neg_flag: false, source: Red };
	/// let channel = ChannelSwizzle { target: Red, data };
	/// assert!(channel.is_noop());
	/// let channel = ChannelSwizzle { target: Blue, data };
	/// assert!(!channel.is_noop());
	/// ```
	pub fn is_noop(&self) -> bool {
		matches!(self, ChannelSwizzle { target, data: ChannelSwizzleData::Source { neg_flag: false, source } } if target == source)
	}
}


#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, FromStr, DekuRead, DekuWrite)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
#[enumeration(case_insensitive)]
#[deku(type = "u8", bits = "2")]
#[repr(usize)]
#[allow(missing_docs)]
pub enum ChannelSwizzleId {
	#[display(fmt = "a")]
	#[enumeration(rename = "A")]
	#[deku(id = "0b00")]
	Alpha = 0x03,
	#[display(fmt = "r")]
	#[enumeration(rename = "R")]
	#[deku(id = "0b01")]
	Red = 0x00,
	#[display(fmt = "g")]
	#[enumeration(rename = "G")]
	#[deku(id = "0b10")]
	Green = 0x01,
	#[display(fmt = "b")]
	#[enumeration(rename = "B")]
	#[deku(id = "0b11")]
	Blue = 0x02,
}


/// Swizzle algorithm for a single channel without its target (see also
/// [`ChannelSwizzle`])
#[derive(Debug, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite)]
#[deku(type = "u8", bits = "1")]
pub enum ChannelSwizzleData {
	/// Copy data from another channel.
	#[deku(id = "0b0")]
	Source {
		/// Negate `source` if true.
		#[deku(bits = "1")]
		neg_flag: bool,
		/// Input texture channel to source from.
		source: ChannelSwizzleId,
	},

	/// Fill the channel with a constant (either all zeroes or all ones).
	#[deku(id = "0b1")]
	Fill {
		#[deku(pad_bits_before = "1")]
		#[allow(missing_docs)]
		value: ChannelSwizzleFill,
	},
}


impl std::str::FromStr for ChannelSwizzleData {
	type Err = PaaError;

	fn from_str(s: &str) -> PaaResult<Self> {
		let mut st = s.trim().to_uppercase();
		st.retain(|c| !c.is_whitespace());

		match st.as_str() {
			s @ ("A" | "R" | "G" | "B") => {
				let result = ChannelSwizzleData::Source {
					neg_flag: false,
					source: s.parse::<ChannelSwizzleId>()
						.map_err(|_| InvalidChannelSwizzleIdString(String::from(s)))?
				};
				Ok(result)
			},

			s @ ("1-A" | "1-R" | "1-G" | "1-B") => {
				let id = s.chars().nth(2).unwrap().to_string();
				let result = ChannelSwizzleData::Source {
					neg_flag: true,
					source: id.parse::<ChannelSwizzleId>()
						.map_err(|_| InvalidChannelSwizzleIdString(String::from(s)))?
				};
				Ok(result)
			},

			s @ ("0" | "1") => {
				let value = match s {
					"0" => ChannelSwizzleFill::Fill00,
					"1" => ChannelSwizzleFill::FillFF,
					_ => unreachable!(),
				};

				Ok(ChannelSwizzleData::Fill { value })
			},

			_ => Err(InvalidSwizzleString(String::from(s))),
		}
	}
}


impl std::fmt::Display for ChannelSwizzleData {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		use ChannelSwizzleData::*;

		match self {
			Source { neg_flag, source } => {
				let neg_str = if *neg_flag { "1-" } else { "" };
				write!(f, "{}{}", neg_str, source)
			},

			Fill { value } => {
				write!(f, "{}", value)
			},
		}
	}
}


#[cfg(feature = "arbitrary")]
impl<'a> Arbitrary<'a> for ChannelSwizzleData {
	fn arbitrary(input: &mut Unstructured) -> ArbitraryResult<Self> {
		let variant: usize = input.int_in_range(1..=2)?;

		let result = match variant {
			1 => {
				let neg_flag: bool = input.arbitrary()?;
				let source: ChannelSwizzleId = input.arbitrary()?;
				ChannelSwizzleData::Source { neg_flag, source }
			},

			2 => {
				let value: ChannelSwizzleFill = input.arbitrary()?;
				ChannelSwizzleData::Fill { value }
			},

			_ => unreachable!(),
		};

		Ok(result)
	}
}


/// The value (ones or zeroes) to fill a channel with while swizzling
#[derive(Debug, Display, Clone, Copy, PartialEq, Eq, DekuRead, DekuWrite)]
#[cfg_attr(feature = "arbitrary", derive(Arbitrary))]
#[deku(type = "u8", bits = "2")]
#[repr(u8)]
pub enum ChannelSwizzleFill {
	/// Fill the channel with `0xFF`s (all ones).
	#[display(fmt = "1")]
	#[deku(id = "0b00")]
	FillFF = 0xFF,
	/// Fill the channel with `0x00`s (all zeroes).
	#[display(fmt = "0")]
	#[deku(id = "0b01")]
	Fill00 = 0x00,
}


#[test]
fn parse_swizzle() {
	for c in ["a", "R", "G", "b"] {
		let src_pos = format!("             {}", c);
		let src_neg = format!("  1 -  {} ", c);
		assert_eq!(src_pos.parse::<ChannelSwizzleData>().unwrap(), ChannelSwizzleData::Source { neg_flag: false, source: c.parse::<ChannelSwizzleId>().unwrap() });
		assert_eq!(src_neg.parse::<ChannelSwizzleData>().unwrap(), ChannelSwizzleData::Source { neg_flag: true, source: c.parse::<ChannelSwizzleId>().unwrap() });
	};
	assert_eq!(" 0 ".parse::<ChannelSwizzleData>().unwrap(), ChannelSwizzleData::Fill{ value: ChannelSwizzleFill::Fill00 });
	assert_eq!("1   ".parse::<ChannelSwizzleData>().unwrap(), ChannelSwizzleData::Fill{ value: ChannelSwizzleFill::FillFF });
}


/// `[TODO]`
#[allow(rustdoc::broken_intra_doc_links)]
#[derive(Debug, Display, Clone, PartialEq, Eq)]
pub struct TextureMacro {
	/// `[TODO]`
	pub text: BString,
}


#[cfg(feature = "arbitrary")]
impl<'a> Arbitrary<'a> for TextureMacro {
	fn arbitrary(input: &mut Unstructured) -> ArbitraryResult<Self> {
		Ok(TextureMacro { text: BString::from(<Vec<u8> as Arbitrary>::arbitrary(input)?) })
	}
}


trait ExtendExt: Extend<u8> {
	/// Convenience function which extends an [`std::iter::Extend<u8>`] with a
	/// [`byteorder::ByteOrder`]-encoded integer.
	fn extend_with_uint<B: ByteOrder, T: Into<u64>, const N: usize>(&mut self, v: T) {
		let mut buf = vec![0u8; N];
		B::write_uint(&mut buf[..], v.into(), N);
		self.extend(buf.into_iter());
	}
}


impl<T> ExtendExt for T where T: Extend<u8> {}


#[test]
fn test_extend_with_uint() {
	let mut dest: Vec<u8> = vec![];

	dest.extend_with_uint::<LittleEndian, _, 2>(1234u16);
	assert_eq!(dest, vec![0xD2, 0x04]);

	dest.extend_with_uint::<LittleEndian, _, 3>(1234u32);
	assert_eq!(dest, vec![0xD2, 0x04, 0xD2, 0x04, 0x00]);

	dest.extend_with_uint::<BigEndian, _, 4>(5678u32);
	assert_eq!(dest, vec![0xD2, 0x04, 0xD2, 0x04, 0x00, 0x00, 0x00, 0x16, 0x2E]);
}


trait ReadExt: Read {
	const SINGLE_READ_SIZE: usize = 64;

	fn read_exact_buffered(&mut self, len: usize) -> PaaResult<Vec<u8>> {
		let mut data: Vec<u8> = Vec::with_capacity(len);
		let mut total = 0usize;

		loop {
			if total == len {
				break;
			};

			let bufsize = std::cmp::min(Self::SINGLE_READ_SIZE, len-total);
			let mut buf = vec![0u8; bufsize];
			self.read_exact(&mut buf)?;
			data.extend(&buf[..]);
			total += bufsize;
		};

		Ok(data)
	}
}


impl<T> ReadExt for T where T: Read { }


#[test]
fn test_read_exact_buffered() {
	let mut input = Cursor::new(vec![0x41u8, 0x42, 0x43, 0x44, 0x45, 0x46]);
	assert_eq!(input.read_exact_buffered(1).unwrap(), vec![0x41u8]);
	assert_eq!(input.read_exact_buffered(2).unwrap(), vec![0x42u8, 0x43]);
	assert_eq!(input.read_exact_buffered(3).unwrap(), vec![0x44u8, 0x45, 0x46]);
}


fn get_additive_i32_cksum(_: &[u8]) -> i32 {
	0
}


#[test]
fn assert_traits() {
	use std::fmt::{Debug, Display};
	use std::error::Error;
	use std::panic::{UnwindSafe, RefUnwindSafe};

	assert_impl_all!(PaaError: Debug, Display, Error, Send, Sync, UnwindSafe, RefUnwindSafe);
}
