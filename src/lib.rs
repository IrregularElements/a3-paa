// Currently implemented
// =====================
// - DXT PAAs with LZO compression
//
// [TODO]
// ======
// - Add index palette support
// - Fix PaaImage::to_bytes
// - Fix LZSS: it's underflowing or overflowing on decompression; looks like an incorrect algorithm
// - Add RLE compression
// - Add image-rs decoding/encoding via PaaDecoder / PaaEncoder
// - Describe PAA in module-level documentation
// - When done, remove Seek from PaaMipmap methods


#![feature(derive_default_enum)]
#![feature(seek_stream_len)]
#![feature(let_chains)]


// let_chains
#![allow(incomplete_features)]


//! This crate provides methods for reading and writing the Bohemia Interactive
//! PAA (PAX) image format.  The main source of information on PAA is the [Biki],
//! which is complemented by the [PMC Editing Wiki].
//!
//! [Biki]: https://community.bistudio.com/wiki/PAA_File_Format
//! [PMC Editing Wiki]: https://pmc.editing.wiki/doku.php?id=arma:file_formats:paa


use std::fmt::Debug;
use std::io::{Read, Seek, SeekFrom, Cursor, ErrorKind};
use std::iter::Extend;
use std::default::Default;

use static_assertions::const_assert;
use bstr::BString;
use byteorder::{LittleEndian, ByteOrder, ReadBytesExt};
use derive_more::{Display, Error};

use PaaError::*;

#[cfg(test)]
use byteorder::BigEndian;


/// [`std::result::Result`] parameterized with [`PaaError`].
pub type PaaResult<T> = std::result::Result<T, PaaError>;


/// `a3_paa`'s [`std::error::Error`] implementation.
#[derive(Debug, Display, Error, Clone)]
pub enum PaaError {
	/// A function that reads from [`std::io::Read`] encountered early EOF.
	#[display(fmt = "UnexpectedEof({:?})", _0)]
	UnexpectedEof(#[error(ignore)] std::io::ErrorKind),

	/// Attempted to read a PAA image with incorrect magic bytes.
	#[display(fmt = "UnknownPaaType({:?})", _0)]
	UnknownPaaType(#[error(ignore)] [u8; 2]),

	/// Attempted to read a Tagg which does not start with the "GGAT" signature.
	UnexpectedTaggSignature,

	/// Attempted to read a Tagg with unknown name.
	#[display(fmt = "UnknownTaggType({:?})", _0)]
	UnknownTaggType(#[error(ignore)] [u8; 4]),

	/// Attempted to read a Tagg with unexpected indicated payload size.
	UnexpectedTaggDataSize,

	/// Attempted to read a [`Tagg::Flag`] with unexpected transparency value.
	UnknownTransparencyValue(#[error(ignore)] u8),

	/// [`PaaPalette::to_bytes`] received a palette with number of colors
	/// overflowing a [`u16`][`std::primitive::u16`].
	PaletteTooLarge,

	/// Mipmap returned by [`PaaMipmap::read_from`] or
	/// [`PaaMipmap::read_from_until_eof`] had zero width or zero height.
	EmptyMipmap,

	/// Mipmap start offset (as indicated in the file) is beyond EOF.
	MipmapOffsetBeyondEof,

	/// Some or all mipmap data (as indicated by mipmap data length) is beyond
	/// EOF.
	MipmapDataBeyondEof,

	/// Input mipmap dimensions higher than 32768.
	MipmapTooLarge,

	/// Mipmap dimensions not multiple of 2 or less than 4.
	UnexpectedMipmapDimensions,

	/// The [`PaaImage`] passed to [`PaaImage::to_bytes`] contained a
	/// [fallible][`PaaMipmapContainer::Fallible`] container variant.
	MipmapErrorDuringEncoding,

	/// A checked arithmetic operation triggered an unexpected under/overflow.
	CorruptedData,

	/// DXT-LZO decompression failed.
	LzoError(/*MinilzoError*/ #[error(ignore)] String),

	LzssCompressError(#[error(ignore)] String),

	/// LZSS decompression failed.
	LzssDecompressError(#[error(ignore)] String),

	/// [`PaaMipmap::read_from`] was passed an LZSS-compressed [`PaaMipmap`]
	/// with incorrect additive checksum, or LZSS decompression resulted in
	/// incorrect data.
	LzssWrongChecksum,

	/// A function that writes to [`std::io::Write`] encountered an I/O error.
	#[display(fmt = "UnexpectedWriteError({:?})", _0)]
	UnexpectedWriteError(#[error(ignore)] std::io::ErrorKind),

	/// Attempted to write a PAA image with more than 16 mipmaps.
	TooManyMipmaps(#[error(ignore)] usize),
}


macro_rules! debug_trace {
	($($arg : tt) *) => {
		if cfg!(debug_assertions) {
			log::trace!($($arg)*);
		};
	}
}


/// A wrapper around [`PaaImage::mipmaps`]; methods that read a `PaaImage`
/// return `Fallible`; methods that write a `PaaImage` only accept `Infallible`.
///
/// The exact way to convert between the two variants is up to the user;
/// the most obvious idiom is to [collect][`PaaMipmapContainer::collect`] the
/// inner vector of [`Fallible`][`PaaMipmapContainer::Infallible`] as
/// `PaaResult<Vec<PaaMipmap>>`.
#[derive(Debug)]
pub enum PaaMipmapContainer {
	Fallible(Vec<PaaResult<PaaMipmap>>),
	Infallible(Vec<PaaMipmap>),
}


impl Default for PaaMipmapContainer {
	fn default() -> Self {
		Self::Infallible(vec![])
	}
}


impl PaaMipmapContainer {
	pub fn collect(self) -> PaaResult<Vec<PaaMipmap>> {
		match self {
			Self::Infallible(v) => PaaResult::Ok(v),
			Self::Fallible(v) => v.into_iter().collect(),
		}
	}

	pub fn collect_fallible(self) -> Vec<PaaResult<PaaMipmap>> {
		match self {
			Self::Infallible(v) => v.into_iter().map(PaaResult::Ok).collect(),
			Self::Fallible(v) => v,
		}
	}
}


#[derive(Default, Debug)]
pub struct PaaImage {
	pub paatype: PaaType,
	pub taggs:   Vec<Tagg>,
	pub offsets: Vec<u32>,
	pub palette: Option<PaaPalette>,
	pub mipmaps: PaaMipmapContainer,
}


impl PaaImage {
	pub fn read_from<R: Read + Seek>(input: &mut R) -> PaaResult<Self> {
		let stream_len = input.stream_len().map_err(|e| UnexpectedEof(e.kind()))?;

		// [TODO] Index palette support
		let mut paatype_bytes = [0u8; 2];
		input.read_exact(&mut paatype_bytes).map_err(|e| UnexpectedEof(e.kind()))?;
		let paatype = PaaType::from_bytes(&paatype_bytes)
			.ok_or(UnknownPaaType(paatype_bytes))?;

		debug_trace!("PaaType: {:?}", paatype);

		let mut offs = vec![0u32; 0];

		let mut taggs: Vec<Tagg> = Vec::with_capacity(10);

		// Read TAGGs
		loop {
			let stream_position = input.stream_position().unwrap();
			debug_trace!("Seek position: {:?}", stream_position);

			let tagghead = Tagg::read_head_from(input);
			debug_trace!("TAGG head: {:?}", tagghead);

			match tagghead {
				Ok((taggtype, payload_length)) => {
					let mut data = vec![0u8; payload_length as usize];
					input.read_exact(&mut data[..]).map_err(|e| UnexpectedEof(e.kind()))?;
					let tagg = Tagg::from_name_and_payload(&*taggtype, &data[..])?;

					if let Tagg::Offs { ref offsets } = &tagg {
						debug_trace!("Reading mipmap offsets from OFFSTAGG: {:?}", offsets);
						offs = offsets.clone();
					}

					taggs.push(tagg);
				},

				Err(e) => {
					match e {
						UnknownTaggType(_) | UnexpectedTaggSignature => {
							debug_trace!("No more taggs");
							input.seek(SeekFrom::Current(-12)).unwrap();
							break;
						},

						_ => Err(e),
					}?;
				},
			}
		}

		let mut palette_data = [0u8; 2];
		input.read_exact(&mut palette_data).map_err(|e| UnexpectedEof(e.kind()))?;

		if palette_data != [0x00, 0x00] {
			return Err(UnknownPaaType(PaaType::PAXTYPE_IPAL_BYTES));
		}

		let stream_position = input.stream_position().unwrap();
		debug_trace!("Seek position: {:?}", stream_position);

		let mipmaps = if offs.is_empty() {
			PaaMipmap::read_from_until_eof(input, paatype)
		} else {
			offs.iter().enumerate().map(|(_idx, offset)| {
				if (*offset).checked_add(4).ok_or(CorruptedData)? >= stream_len as u32 {
					return Err(MipmapOffsetBeyondEof);
				}

				input.seek(SeekFrom::Start(*offset as u64)).unwrap();

				PaaMipmap::read_from(input, paatype)
			})
				.collect::<Vec<PaaResult<PaaMipmap>>>()
		};

		let image = PaaImage { paatype, taggs, offsets: offs, palette: None, mipmaps: PaaMipmapContainer::Fallible(mipmaps) };

		Ok(image)
	}


	/// Ignores input Taggs::Offs and regenerates offsets based on actual mipmap
	/// data.
	pub fn to_bytes(&self) -> PaaResult<Vec<u8>> {
		let mut buf: Vec<u8> = Vec::with_capacity(10_000_000);

		buf.extend(self.paatype.to_bytes());

		for ref t in self.taggs.iter() {
			if let Tagg::Offs { .. } = t {
				continue;
			}

			buf.extend(t.to_bytes());
		}

		let offs_position = buf.len();

		// Placeholder offsets Tagg to be populated later
		buf.extend(Tagg::Offs { offsets: vec![] }.to_bytes());

		if let Some(p) = &self.palette {
			buf.extend(p.to_bytes()?);
		}
		else {
			buf.extend([0u8, 0]);
		}

		let mipmaps_position = buf.len() as u32;

		let mipmaps = if let PaaMipmapContainer::Infallible(mipmaps) = &self.mipmaps {
			Ok(mipmaps)
		}
		else {
			Err(MipmapErrorDuringEncoding)
		}?;

		let mipmap_blocks = mipmaps
			.iter()
			.map(|m| m.to_bytes())
			.collect::<PaaResult<Vec<Vec<u8>>>>()?;

		let mipmap_block_offsets: Vec<u32> = mipmap_blocks
			.iter()
			.scan(0, |acc, b| {
				debug_trace!("mipmap_block_offsets: current={} b.len()={} offset={}", *acc, b.len(), *acc + mipmaps_position);
				let current = *acc;
				*acc += b.len() as u32;
				Some(current + mipmaps_position)
			})
			.collect::<Vec<u32>>();

		let new_offs = Tagg::Offs { offsets: mipmap_block_offsets }.to_bytes();

		buf.splice(offs_position..(offs_position + new_offs.len() + 1), new_offs);

		for m in mipmap_blocks {
			buf.extend(m);
		}

		buf.extend([0u8; 6]);

		Ok(buf)
	}
}


#[derive(Default, Debug, Clone, Copy)]
pub enum PaaType {
	Dxt1,

	#[deprecated]
	Dxt2,

	#[deprecated]
	Dxt3,

	#[deprecated]
	Dxt4,

	#[default]
	Dxt5,

	/// RGBA 4:4:4:4
	Rgba4,

	/// RGBA 5:5:5:1
	Rgba5,

	/// RGBA 8:8:8:8
	Rgba8,

	/// 8 bits alpha, 8 bits grayscale
	Gray,

	/// 1 byte (offset into the index palette, which contains BGR 8:8:8)
	#[deprecated = "[TODO] Index palette format is not implemented"]
	IndexPalette,
}


impl PaaType {
	// See `int __stdcall sub_4276E0(void *Block, int)` (ImageToPAA v1.0.0.3).
	const PAXTYPE_DXT1_BYTES: [u8; 2] = [0x01, 0xFF];
	const PAXTYPE_DXT2_BYTES: [u8; 2] = [0x02, 0xFF];
	const PAXTYPE_DXT3_BYTES: [u8; 2] = [0x03, 0xFF];
	const PAXTYPE_DXT4_BYTES: [u8; 2] = [0x04, 0xFF];
	const PAXTYPE_DXT5_BYTES: [u8; 2] = [0x05, 0xFF];
	const PAXTYPE_RGB4_BYTES: [u8; 2] = [0x44, 0x44];
	const PAXTYPE_RGB5_BYTES: [u8; 2] = [0x55, 0x15];
	const PAXTYPE_RGB8_BYTES: [u8; 2] = [0x88, 0x88];
	const PAXTYPE_GRAY_BYTES: [u8; 2] = [0x80, 0x80];
	const PAXTYPE_IPAL_BYTES: [u8; 2] = [0x47, 0x47];


	pub fn from_bytes(value: &[u8; 2]) -> Option<Self> {
		#[allow(deprecated)]
		match *value {
			Self::PAXTYPE_DXT1_BYTES => Some(Self::Dxt1),
			Self::PAXTYPE_DXT2_BYTES => Some(Self::Dxt2),
			Self::PAXTYPE_DXT3_BYTES => Some(Self::Dxt3),
			Self::PAXTYPE_DXT4_BYTES => Some(Self::Dxt4),
			Self::PAXTYPE_DXT5_BYTES => Some(Self::Dxt5),
			Self::PAXTYPE_RGB4_BYTES => Some(Self::Rgba4),
			Self::PAXTYPE_RGB5_BYTES => Some(Self::Rgba5),
			Self::PAXTYPE_RGB8_BYTES => Some(Self::Rgba8),
			Self::PAXTYPE_GRAY_BYTES => Some(Self::Gray),
			Self::PAXTYPE_IPAL_BYTES => Some(Self::IndexPalette),
			_ => None,
		}
	}


	pub const fn to_bytes(&self) -> [u8; 2] {
		use PaaType::*;

		#[allow(deprecated)]
		match self {
			Dxt1 => Self::PAXTYPE_DXT1_BYTES,
			Dxt2 => Self::PAXTYPE_DXT2_BYTES,
			Dxt3 => Self::PAXTYPE_DXT3_BYTES,
			Dxt4 => Self::PAXTYPE_DXT4_BYTES,
			Dxt5 => Self::PAXTYPE_DXT5_BYTES,
			Rgba4 => Self::PAXTYPE_RGB4_BYTES,
			Rgba5 => Self::PAXTYPE_RGB5_BYTES,
			Rgba8 => Self::PAXTYPE_RGB8_BYTES,
			Gray => Self::PAXTYPE_GRAY_BYTES,
			IndexPalette => Self::PAXTYPE_IPAL_BYTES,
		}
	}


	pub const fn predict_size(&self, width: u16, height: u16) -> usize {
		use PaaType::*;

		const_assert!(std::mem::size_of::<usize>() >= 4);

		let mut result = width as usize * height as usize;

		match self {
			Dxt1 => { result /= 2 },
			#[allow(deprecated)]
			IndexPalette | Dxt2 | Dxt3 | Dxt4 | Dxt5 => (),
			Rgba4 | Rgba5 | Gray => { result *= 2 },
			Rgba8 => { result *= 4 },
		}

		result
	}


	#[allow(deprecated)]
	pub const fn is_dxtn(&self) -> bool {
		use PaaType::*;
		matches!(self, Dxt1 | Dxt2 | Dxt3 | Dxt4 | Dxt5)
	}
}


#[derive(Debug, Clone)]
pub enum Tagg {
	Avgc {
		rgba: u32
	},

	Maxc {
		rgba: u32
	},

	Flag {
		transparency: Transparency
	},

	Swiz {
		swizzle: u32
	},

	Proc {
		text: BString
	},

	Offs {
		offsets: Vec<u32>
	},
}


impl Tagg {
	/// Serialize a Tagg into Vec<u8>.
	pub fn to_bytes(&self) -> Vec<u8> {
		const U32_SIZE: u32 = std::mem::size_of::<u32>() as u32;

		let mut bytes: Vec<u8> = Vec::with_capacity(256);
		bytes.extend("GGAT".as_bytes());
		bytes.extend(self.as_taggname().as_bytes());

		match self {
			Self::Avgc { rgba } => {
				extend_with_uint::<LittleEndian,Vec<u8>, 4, _>(&mut bytes, U32_SIZE);
				extend_with_uint::<LittleEndian,Vec<u8>, 4, _>(&mut bytes, *rgba);
			},

			Self::Maxc { rgba } => {
				extend_with_uint::<LittleEndian,Vec<u8>, 4, _>(&mut bytes, U32_SIZE);
				extend_with_uint::<LittleEndian,Vec<u8>, 4, _>(&mut bytes, *rgba);
			},

			Self::Flag { transparency } => {
				extend_with_uint::<LittleEndian,Vec<u8>, 4, _>(&mut bytes, U32_SIZE);
				let trans = <u8 as From<&Transparency>>::from(transparency);
				bytes.extend([trans, 0, 0, 0]);
			},

			Self::Swiz { swizzle } => {
				extend_with_uint::<LittleEndian,Vec<u8>, 4, _>(&mut bytes, U32_SIZE);
				extend_with_uint::<LittleEndian,Vec<u8>, 4, _>(&mut bytes, *swizzle);
			},

			Self::Proc { text } => {
				let len = (&text[..]).len() as u32;
				extend_with_uint::<LittleEndian,Vec<u8>, 4, _>(&mut bytes, len);
				bytes.extend(&text[..]);
			},

			Self::Offs { offsets } => {
				let len = (16 * std::mem::size_of::<u32>()) as u32;
				extend_with_uint::<LittleEndian,Vec<u8>, 4, _>(&mut bytes, len);

				let mut buf = [0u8; 16*4];
				let mut offsets = offsets.clone();
				if offsets.len() != 16 {
					offsets.resize(16, 0);
				}

				LittleEndian::write_u32_into(&offsets[..], &mut buf);
				bytes.extend(&buf);
			},
		};

		bytes
	}


	/// Read a fixed (12) number of bytes from `input` and validate contained
	/// Tagg metadata: "TAGG" signature, tag name, and payload length.
	/// Returns PaaResult<(name: String, payload_size: u32)>.
	pub fn read_head_from<R: Read>(input: &mut R) -> PaaResult<(String, u32)> {
		let mut tagghead = [0u8; 12];
		input.read_exact(&mut tagghead).map_err(|e| UnexpectedEof(e.kind()))?;

		let taggsig = &tagghead[0..4];

		// "GGAT" signature
		if taggsig != [0x47u8, 0x47, 0x41, 0x54] {
			return Err(UnexpectedTaggSignature);
		}

		let taggname: String = std::str::from_utf8(&tagghead[4..8])
			.map_err(|_| UnknownTaggType((&tagghead[4..8]).try_into().unwrap()))?
			.into();

		if ! Self::is_valid_taggname(&taggname) {
			return Err(UnknownTaggType(taggname.as_bytes().try_into().unwrap()));
		}

		let payload_length = LittleEndian::read_u32(&tagghead[8..12]);

		Ok((taggname, payload_length))
	}


	pub fn from_name_and_payload(taggname: &str, data: &[u8]) -> PaaResult<Self> {
		if taggname.len() != 4 {
			return Err(UnexpectedTaggSignature);
		}

		match taggname {
			"CGVA" => {
				if data.len() != 4 {
					return Err(UnexpectedTaggDataSize);
				}
				let rgba = LittleEndian::read_u32(data);
				Ok(Self::Avgc { rgba })
			},

			"CXAM" => {
				if data.len() != 4 {
					return Err(UnexpectedTaggDataSize);
				}
				let rgba = LittleEndian::read_u32(data);
				Ok(Self::Maxc { rgba })
			},

			"GALF" => {
				if data.len() != 4 {
					return Err(UnexpectedTaggDataSize);
				}

				let trans = data[0];
				let transparency: Transparency = trans.try_into()
					.map_err(|_| UnknownTransparencyValue(trans))?;

				Ok(Self::Flag { transparency })
			},

			"ZIWS" => {
				if data.len() != 4 {
					return Err(UnexpectedTaggDataSize);
				}
				let swizzle = LittleEndian::read_u32(data);
				Ok(Self::Swiz { swizzle })
			},

			"CORP" => {
				let text = BString::from(data);
				Ok(Self::Proc { text })
			},

			"SFFO" => {
				// [NOTE] Offset vectors that are not of length 16 do not
				// apparently occur; however, we do allow them nonetheless
				if data.len() % std::mem::size_of::<u32>() != 0 {
					return Err(UnexpectedTaggDataSize);
				}

				let offset_count = data.len() / std::mem::size_of::<u32>();
				let mut offsets = vec![0u32; offset_count];

				LittleEndian::read_u32_into(data, &mut offsets[..]);

				if let Some(idx) = offsets.iter().position(|x| *x == 0) {
					offsets.truncate(idx);
				}

				Ok(Self::Offs { offsets })
			},

			_ => Err(UnknownTaggType(taggname.as_bytes().try_into().unwrap())),
		}
	}


	/// Return the 4-byte signature (as ASCII String), e.g. "SFFO" for the
	/// offsets Tagg.
	pub fn as_taggname(&self) -> String {
		match self {
			Self::Avgc { .. } => "CGVA",
			Self::Maxc { .. } => "CXAM",
			Self::Flag { .. } => "GALF",
			Self::Swiz { .. } => "ZIWS",
			Self::Proc { .. } => "CORP",
			Self::Offs { .. } => "SFFO",
		}.into()
	}


	/// Check if `name` is a valid 4-character Tagg name as represented in the
	/// file (e.g. "SFFO").
	pub fn is_valid_taggname(name: &str) -> bool {
		matches!(name, "CGVA" | "CXAM" | "GALF" | "ZIWS" | "CORP" | "SFFO")
	}
}


#[derive(Debug, Default, Clone)]
pub enum Transparency {
	None,
	#[default]
	AlphaInterpolated,
	AlphaNotInterpolated,
}


impl TryFrom<u8> for Transparency {
	type Error = ();

	fn try_from(value: u8) -> Result<Self, Self::Error> {
		use Transparency::*;

		match value {
			0 => Ok(None),
			1 => Ok(AlphaInterpolated),
			2 => Ok(AlphaNotInterpolated),
			_ => Err(()),
		}
	}
}


impl From<&Transparency> for u8 {
	fn from(value: &Transparency) -> Self {
		match value {
			Transparency::None => 0,
			Transparency::AlphaInterpolated => 1,
			Transparency::AlphaNotInterpolated => 2,
		}
	}
}


#[derive(Default, Debug)]
pub struct PaaPalette {
	pub triplets: Vec<[u8; 3]>,
}


impl PaaPalette {
	pub fn to_bytes(&self) -> PaaResult<Vec<u8>> {
		const_assert!(std::mem::size_of::<usize>() >= std::mem::size_of::<u16>());

		if self.triplets.len() > u16::MAX as usize {
			return Err(PaletteTooLarge);
		}

		let ntriplets = self.triplets.len() as u16;
		let mut buf: Vec<u8> = Vec::with_capacity(2 + (ntriplets as usize) * 3);

		extend_with_uint::<LittleEndian, _, 2, _>(&mut buf, ntriplets);

		for triplet in self.triplets.iter() {
			buf.extend(triplet);
		}

		Ok(buf)
	}
}


#[derive(Debug, Clone)]
pub struct PaaMipmap {
	pub width: u16,
	pub height: u16,
	pub paatype: PaaType,
	pub compression: PaaMipmapCompression,
	pub data: Vec<u8>,
}


impl PaaMipmap {
	#[allow(deprecated)]
	pub fn read_from<R: Read + Seek>(input: &mut R, paatype: PaaType) -> PaaResult<Self> {
		use PaaType::*;
		use PaaMipmapCompression::*;

		let pos = input.stream_position().unwrap();

		let mut paatype = paatype;
		let mut compression = PaaMipmapCompression::Uncompressed;

		let mut width = input.read_u16::<LittleEndian>().map_err(|e| UnexpectedEof(e.kind()))?;
		let mut height = input.read_u16::<LittleEndian>().map_err(|e| UnexpectedEof(e.kind()))?;

		if width == 0 || height == 0 {
			return Err(EmptyMipmap);
		}

		if width == 1234 && height == 8765 {
			paatype = PaaType::IndexPalette;
			compression = PaaMipmapCompression::Lzss;

			width = input.read_u16::<LittleEndian>().map_err(|e| UnexpectedEof(e.kind()))?;
			height = input.read_u16::<LittleEndian>().map_err(|e| UnexpectedEof(e.kind()))?;
		}

		if width & 0x8000 != 0 && paatype.is_dxtn() {
			compression = PaaMipmapCompression::Lzo;
			width ^= 0x8000;
		}

		const_assert!(std::mem::size_of::<usize>() >= 3);
		let data_len = paatype.predict_size(width, height);
		let data_compressed_len = input.read_uint::<LittleEndian>(3)
			.map_err(|e| UnexpectedEof(e.kind()))? as usize;

		if matches!(paatype, IndexPalette) && !matches!(compression, Lzss) {
			compression = RleBlocks;
		}
		else if matches!(compression, Uncompressed) && data_len != data_compressed_len && !paatype.is_dxtn() {
			compression = Lzss;
		}

		let mut compressed_data_buf: Vec<u8> = vec![0; data_compressed_len];
		input.read_exact(&mut compressed_data_buf).map_err(|e| UnexpectedEof(e.kind()))?;

		let data: Vec<u8> = match compression {
			Uncompressed => {
				compressed_data_buf
			},

			Lzo => {
				decompress_lzo_slice(&compressed_data_buf[..], data_len)?
			},

			Lzss => {
				let split_pos = compressed_data_buf.len().checked_sub(4+1).ok_or(CorruptedData)?;
				let (lzss_slice, checksum_slice) = compressed_data_buf.split_at(split_pos);
				let checksum = LittleEndian::read_i32(checksum_slice);
				let uncompressed_data = decompress_lzss_slice(lzss_slice, data_len)?;

				let calculated_checksum = get_additive_i32_cksum(&uncompressed_data);

				if calculated_checksum != checksum {
					// [FIXME] keeps firing
					//return Err(LzssWrongChecksum);
				}

				uncompressed_data
			},

			RleBlocks => {
				decompress_rleblock_slice(&compressed_data_buf[..])?
			},
		};

		let new_pos = input.stream_position().unwrap();

		debug_trace!("PaaMipmap::read_from: pos={} new_pos={} diff={}", pos, new_pos, new_pos-pos);

		Ok(PaaMipmap { width, height, paatype, compression, data })
	}


	pub fn from_bytes(input: &[u8], paatype: PaaType) -> PaaResult<Self> {
		let mut cursor = Cursor::new(input);
		Self::read_from(&mut cursor, paatype)
	}


	pub fn read_from_until_eof<R: Read + Seek>(input: &mut R, paatype: PaaType) -> Vec<PaaResult<PaaMipmap>> {
		let mut result: Vec<PaaResult<PaaMipmap>> = Vec::with_capacity(8);

		loop {
			let mip = PaaMipmap::read_from(input, paatype);
			let is_eof = matches!(mip, Err(MipmapDataBeyondEof) | Err(EmptyMipmap) | Err(UnexpectedEof(_)));

			result.push(mip);

			if is_eof {
				break;
			}
		}

		result
	}


	pub fn to_bytes(&self) -> PaaResult<Vec<u8>> {
		use PaaType::*;
		use PaaMipmapCompression::*;

		const _64_MIB: usize = 67_108_864;
		let mut bytes: Vec<u8> = Vec::with_capacity(_64_MIB);

		if self.width >= 32768 || self.height >= 32768 {
			return Err(MipmapTooLarge);
		}

		let non_power_of_2 = self.width.count_ones() > 1 || self.height.count_ones() > 1;
		let too_small = self.width < 4 || self.height < 4;

		if non_power_of_2 || too_small {
			return Err(UnexpectedMipmapDimensions);
		}

		let mut width = self.width;
		let mut height = self.height;

		#[allow(deprecated)]
		if let (Lzss, IndexPalette) = (&self.compression, &self.paatype) && !self.is_empty() {
			width = 1234;
			height = 8765;
		}

		if let Lzo = &self.compression && self.paatype.is_dxtn() && !self.is_empty() {
			width ^= 0x8000;
		}

		extend_with_uint::<LittleEndian, _, 2, _>(&mut bytes, width);
		extend_with_uint::<LittleEndian, _, 2, _>(&mut bytes, height);

		debug_trace!("MipMap::to_bytes: after width,height @ {}", bytes.len());

		if self.is_empty() {
			return Ok(bytes);
		}

		#[allow(deprecated)]
		if let (Lzss { .. }, IndexPalette) = (&self.compression, &self.paatype) {
			extend_with_uint::<LittleEndian, _, 2, _>(&mut bytes, self.width);
			extend_with_uint::<LittleEndian, _, 2, _>(&mut bytes, self.height);

			// [TODO] Does the mipmap code on Biki mean that index palette lzss
			// data does not have `byte size[3]`?  I'm thinking probably not but
			// this needs to be tested on old PACs
		}

		debug_trace!("MipMap::to_bytes: after Lzss @ {}", bytes.len());

		let mut compressed_data: Vec<u8> = Vec::with_capacity(std::cmp::min(self.data.len() * 2, 128));

		match &self.compression {
			Uncompressed => {
				compressed_data.extend(&self.data[..]);
			},

			Lzo => {
				let lzo_data = compress_lzo_slice(&self.data[..])?;
				compressed_data.extend(lzo_data);
			},

			Lzss => {
				let lzss_data = compress_lzss_slice(&self.data[..])?;
				compressed_data.extend(lzss_data);

				let cksum = get_additive_i32_cksum(&self.data[..]);
				let mut buf = [0u8; 4];
				LittleEndian::write_i32(&mut buf, cksum);
				compressed_data.extend(buf);
			},

			RleBlocks => {
				let rle_data = compress_rleblock_slice(&self.data[..]);
				compressed_data.extend(rle_data);
			},
		}

		extend_with_uint::<LittleEndian, _, 3, u32>(&mut bytes, compressed_data.len() as u32);
		debug_trace!("MipMap::to_bytes: after length @ {}", bytes.len());
		bytes.extend(&compressed_data[..]);
		debug_trace!("MipMap::to_bytes: after data @ {}", bytes.len());

		Ok(bytes)
	}


	pub fn is_empty(&self) -> bool {
		self.width == 0 || self.height == 0
	}
}


/// The algorithm compressing the data of a given mipmap.
#[derive(Debug, Copy, Clone)]
pub enum PaaMipmapCompression {
	Uncompressed,

	Lzo,

	Lzss,

	RleBlocks,
}


#[derive(Debug, Default)]
pub enum PaaTextureType {
	Colormap,

	#[default]
	ColormapWithAlpha,

	NormalMap,
	NormalMapSpecularWithAlpha,
	NormalMapSpecularHighQualityWithAlpha,
	NormalMapFaded,
	NormalMapFadedHighQuality,
	NormalMapWithAlphaNoise,
	NormalMapHighQuality,
	NormalMapHighQualityTwoComponentDxt5,

	DetailTexture,
	ColoredDetailTexture,
	MultiplyColorMap,

	MacroTexture,

	AmbientShadowTexture,
	AmbientShadowTextureDiffuse,

	SpecularMap,
	SpecularMapOptimized,
	SpecularMapOptimizedDetail,

	SkyTexture,

	TerrainLayerColorMap,
}


impl<'a> TryFrom<&'a str> for PaaTextureType {
	type Error = ();

	fn try_from(value: &'a str) -> Result<Self, Self::Error> {
		use PaaTextureType::*;

		match &*value.to_lowercase() {
			"co" => Ok(Colormap),
			"ca" => Ok(ColormapWithAlpha),

			"no" | "normalmap" => Ok(NormalMap),
			"ns" => Ok(NormalMapSpecularWithAlpha),
			"nshq" => Ok(NormalMapSpecularHighQualityWithAlpha),
			"nof" => Ok(NormalMapFaded),
			"nofhq" => Ok(NormalMapFadedHighQuality),
			"non" => Ok(NormalMapWithAlphaNoise),
			"nohq" => Ok(NormalMapHighQuality),
			"novhq" => Ok(NormalMapHighQualityTwoComponentDxt5),

			_ => Err(()),
		}
	}
}


//impl Into<String> for PaaTextureType {
//}


impl PaaTextureType {
	pub fn has_alpha(&self) -> bool {
		use PaaTextureType::*;
		matches!(self,
			ColormapWithAlpha | NormalMapSpecularWithAlpha |
			NormalMapSpecularHighQualityWithAlpha | NormalMapWithAlphaNoise)
	}


	pub fn is_colormap(&self) -> bool {
		use PaaTextureType::*;
		matches!(self, Colormap | ColormapWithAlpha)
	}


	pub fn is_normalmap(&self) -> bool {
		use PaaTextureType::*;
		matches!(self,
			NormalMap | NormalMapSpecularWithAlpha |
			NormalMapSpecularHighQualityWithAlpha | NormalMapFaded |
			NormalMapFadedHighQuality | NormalMapWithAlphaNoise |
			NormalMapHighQuality | NormalMapHighQualityTwoComponentDxt5)
	}


	pub fn is_detailmap(&self) -> bool {
		use PaaTextureType::*;
		matches!(self, DetailTexture | ColoredDetailTexture | MultiplyColorMap)
	}
}


pub struct PaaDecoder {
}


impl PaaDecoder {
}


pub struct PaaEncoder {
}


impl PaaEncoder {
}


/// A convenience function which extends an [`std::iter::Extend<u8>`] with a
/// [`byteorder::ByteOrder`]-encoded integer.
pub fn extend_with_uint<B, E, const N: usize, T>(e: &mut E, v: T)
	where
		B: ByteOrder,
		E: Extend<u8>,
		T: Into<u64>,
{
	let mut buf = vec![0u8; N];
	B::write_uint(&mut buf[..], v.into(), N);
	e.extend(buf.into_iter());
}


#[test]
fn test_extend_with_uint() {
	let mut dest = vec![];

	extend_with_uint::<LittleEndian, _, 2, _>(&mut dest, 1234u16);
	assert_eq!(dest, vec![0xD2, 0x04]);

	extend_with_uint::<LittleEndian, _, 3, _>(&mut dest, 1234u32);
	assert_eq!(dest, vec![0xD2, 0x04, 0xD2, 0x04, 0x00]);

	extend_with_uint::<BigEndian, _, 4, _>(&mut dest, 5678u32);
	assert_eq!(dest, vec![0xD2, 0x04, 0xD2, 0x04, 0x00, 0x00, 0x00, 0x16, 0x2E]);
}


pub fn get_additive_i32_cksum(input: &[u8]) -> i32 {
	input.iter().fold(0i32, |a, b| { a.wrapping_add(*b as i32) })
}


pub fn decompress_lzo_slice(input: &[u8], dst_len: usize) -> PaaResult<Vec<u8>> {
	let lzo = minilzo_rs::LZO::init().unwrap();
	lzo.decompress_safe(input, dst_len).map_err(|e| LzoError(format!("{:?}", e)))
}


#[allow(unused_variables)]
pub fn decompress_lzss_slice(input: &[u8], dst_len: usize) -> PaaResult<Vec<u8>> {
	Ok(input.to_vec())
}


pub fn decompress_rleblock_slice(input: &[u8]) -> PaaResult<Vec<u8>> {
	const_assert!(std::mem::size_of::<usize>() >= 1);

	let mut cursor = Cursor::new(input);
	let mut buf: Vec<u8> = Vec::with_capacity(input.len() * 2);

	while let Ok(flag) = cursor.read_u8() {
		let data = if flag & 0x80 != 0 {
			let n = (flag ^ 0x80) + 1;
			let byte = cursor.read_u8().map_err(|e| UnexpectedEof(e.kind()))?;
			vec![byte; n as usize]
		}
		else {
			let n = flag + 1;
			let mut data = vec![0u8; n as usize];
			cursor.read_exact(&mut data).map_err(|_| UnexpectedEof(ErrorKind::UnexpectedEof))?;
			data
		};

		buf.extend(data);
	}

	Ok(buf)
}

#[test]
fn test_decompress_rleblock_slice() {
	let data = vec![0x80u8, 0x41, 0x02, 0x00, 0x00, 0x00, 0x82, 0x41];
	let wanted = vec![0x41u8, 0x00, 0x00, 0x00, 0x41, 0x41, 0x41];

	let actual = decompress_rleblock_slice(&data[..]).unwrap();

	assert_eq!(wanted, actual);
}


pub fn compress_lzo_slice(input: &[u8]) -> PaaResult<Vec<u8>> {
	let mut lzo = minilzo_rs::LZO::init().unwrap();
	lzo.compress(input).map_err(|e| LzoError(format!("{:?}", e)))
}


pub fn compress_lzss_slice(input: &[u8]) -> PaaResult<Vec<u8>> {
	type MyLzss = lzss::Lzss<12, 4, 0x20, {1 <<12}, {2 << 12}>;

	let bufsize = std::cmp::min(128, input.len() + MyLzss::MIN_OFFSET);
	let mut buf: Vec<u8> = vec![0; bufsize];

	let result = MyLzss::compress(lzss::SliceReader::new(input), lzss::SliceWriter::new(&mut buf))
		.map_err(|e| LzssCompressError(format!("{}", e)))?;
	buf.truncate(result);
	Ok(buf)
}


#[allow(unused_variables)]
pub fn compress_rleblock_slice(input: &[u8]) -> Vec<u8> {
	todo!()
}
