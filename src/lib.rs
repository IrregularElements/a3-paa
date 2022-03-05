// Currently implemented
// =====================
// - DXT PAAs with LZO compression
//
// [TODO]
// ======
// - Add index palette support
// - Fix LZO: re-compressed data is different
// - Add RLE compression
// - Add image-rs decoding/encoding via PaaDecoder / PaaEncoder
// - Describe PAA in module-level documentation
// - When done, remove Seek from PaaMipmap methods


#![allow(deprecated)]
#![cfg_attr(doc, feature(doc_cfg))]


#![doc = include_str!("../README.md")]


use std::fmt::Debug;
use std::io::{Read, Seek, SeekFrom, Cursor};
use std::iter::Extend;
use std::default::Default;

use static_assertions::const_assert;
use derive_more::{Display, Error};
#[cfg(feature = "fuzz")] use arbitrary::{Arbitrary, Unstructured, Result as ArbitraryResult};
use deku::prelude::*;
use byteorder::{LittleEndian, ByteOrder, ReadBytesExt};
#[cfg(test)] use byteorder::BigEndian;
use bstr::BString;
use segvec::SegVec;
use image::{RgbaImage, Pixel};
use squish::Format as SquishFormat;
use bohemia_compression::*;

use PaaError::*;



macro_rules! debug_trace {
	($fmt:expr) => {
		if cfg!(debug_assertions) {
			log::trace!(concat!("debug_trace: ", $fmt));
		};
	};

	($fmt:expr, $($arg:tt)*) => {
		if cfg!(debug_assertions) {
			log::trace!(concat!("debug_trace: ", $fmt), $($arg)*);
		};
	};
}


/// [`std::result::Result`] parameterized with [`PaaError`].
pub type PaaResult<T> = std::result::Result<T, PaaError>;


/// `a3_paa`'s [`std::error::Error`] implementation.
#[derive(Debug, Display, Error, Clone)]
pub enum PaaError {
	/// A function that reads from [`std::io::Read`] encountered early EOF.
	#[display(fmt = "Unexpected end of input file")]
	UnexpectedEof,

	#[display(fmt = "Unexpected I/O error: {}", _0)]
	UnexpectedIoError(#[error(ignore)] std::io::ErrorKind),

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

	/// [`PaaPalette::as_bytes`] received a palette with number of colors
	/// overflowing a [`u16`][std::primitive::u16].
	#[display(fmt = "Received a palette with number of colors overflowing a u16 while encoding")]
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

	/// Input mipmap dimensions higher than 32768.
	#[display(fmt = "Received a mipmap with one or both dimensions larger than 32768 while encoding")]
	MipmapTooLarge,

	/// Mipmap dimensions not multiple of 2 or less than 4.
	#[display(fmt = "DXTn mipmap dimensions not multiple of 2 or less than 4")]
	UnexpectedMipmapDimensions,

	/// Uncompressed mipmap data is not of the same size as computed by
	/// [`PaaType::predict_size`].  Enum members are width, height and
	/// [`predict_size`][PaaType::predict_size] result.
	#[error(ignore)]
	#[display(fmt = "Uncompressed mipmap data is not the same size as computed from dimensions (predict_size({}x{}) = {})", _0, _1, _2)]
	UnexpectedMipmapDataSize(u16, u16, usize),

	/// The [`PaaImage`] passed to [`PaaImage::as_bytes`] contained mipmap errors.
	#[display(fmt = "The PaaImage passed to PaaImage::as_bytes contained mipmap errors")]
	InputMipmapErrorWhileEncoding(usize, Box<PaaError>),

	/// [`PaaMipmap::as_bytes`] failed.
	#[display(fmt = "PaaMipmap::as_bytes failed")]
	MipmapErrorWhileSerializing(Box<PaaError>),

	/// A checked arithmetic operation triggered an unexpected under/overflow.
	#[display(fmt = "A checked arithmetic operation triggered an unexpected under/overflow")]
	CorruptedData,

	/// An error occurred while uncompressing RLE data (this likely means the
	/// data is incomplete).
	#[display(fmt = "An error occurred while uncompressing RLE data (compressed data likely truncated)")]
	RleError(BcError),

	/// DXT-LZO de/compression failed.
	#[display(fmt = "DXT-LZO decompression failed: {}", _0)]
	LzoError(/*MinilzoError*/ #[error(ignore)] String),

	/// LZSS decompression failed.
	#[display(fmt = "LZSS decompression failed")]
	LzssDecompressError,

	/// [`PaaMipmap::read_from`] was passed an LZSS-compressed [`PaaMipmap`]
	/// with incorrect additive checksum, or LZSS decompression resulted in
	/// incorrect data.
	#[display(fmt = "LZSS checksum present in mipmap differs from the checksum computed on uncompressed data")]
	LzssWrongChecksum,

	/// A function that writes to [`std::io::Write`] encountered an I/O error.
	#[display(fmt = "A function that writes to std::io::Write encountered an I/O error: ({:?})", _0)]
	UnexpectedWriteError(#[error(ignore)] std::io::ErrorKind),

	/// Attempted to write a PAA image with more than 16 mipmaps.
	#[display(fmt = "Attempted to write a PAA image with more than 16 mipmaps: {}", _0)]
	TooManyMipmaps(#[error(ignore)] usize),

	#[display(fmt = "Mipmap index out of range")]
	MipmapIndexOutOfRange,
}


impl From<std::io::Error> for PaaError {
	fn from(error: std::io::Error) -> Self {
		match error.kind() {
			std::io::ErrorKind::UnexpectedEof => {
				UnexpectedEof
			},

			kind => {
				UnexpectedIoError(kind)
			},
		}
	}
}


#[derive(Default, Debug, Clone)]
pub struct PaaImage {
	pub paatype: PaaType,
	pub taggs:   Vec<Tagg>,
	pub offsets: Vec<u32>,
	pub palette: Option<PaaPalette>,
	pub mipmaps: Vec<PaaResult<PaaMipmap>>,
}


impl PaaImage {
	/// Read a [`PaaImage`][Self] from an [`std::io::Read`].
	pub fn read_from<R: Read + Seek>(input: &mut R) -> PaaResult<Self> {
		// [TODO] Index palette support
		let paatype_bytes: [u8; 2] = read_exact_buffered(input, 2)?
			.try_into()
			.expect("Could not convert paatype_bytes (this is a bug)");
		let (_, paatype) = PaaType::from_bytes((&paatype_bytes, 0))
			.map_err(|_| UnknownPaaType(paatype_bytes))?;

		debug_trace!("PaaType: {:?}", paatype);

		let mut offs = vec![0u32; 0];

		let mut taggs: Vec<Tagg> = Vec::with_capacity(10);

		// Read TAGGs
		loop {
			let stream_position = input.stream_position().unwrap();
			debug_trace!("Seek position: {:?}", stream_position);

			let mut tagghead_data = [0u8; 12];
			input.read_exact(&mut tagghead_data)?;

			let tagghead = Tagg::try_head_from(&tagghead_data);
			debug_trace!("TAGG head: {:?}", tagghead);

			match tagghead {
				Ok((taggtype, payload_length)) => {
					let data = read_exact_buffered(input, payload_length as usize)?;
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

		let palette = PaaPalette::read_from(input)?;

		if palette.is_some() {
			return Err(UnknownPaaType(PaaType::IndexPalette.to_bytes().unwrap().try_into().unwrap()));
		}

		let stream_position = input.stream_position().unwrap();
		debug_trace!("Seek position: {:?}", stream_position);

		let mipmaps = if offs.is_empty() {
			PaaMipmap::read_from_until_eof(input, paatype)
		} else {
			offs.iter().enumerate().map(|(_idx, offset)| {
				let _ = (*offset).checked_add(4).ok_or(CorruptedData)?;

				input.seek(SeekFrom::Start(*offset as u64)).map_err(|e| {
					match e.kind() {
						std::io::ErrorKind::UnexpectedEof => {
							MipmapOffsetBeyondEof
						},

						e => UnexpectedIoError(e)
					}
				})?;

				PaaMipmap::read_from(input, paatype)
			})
				.collect::<Vec<PaaResult<PaaMipmap>>>()
		};

		let image = PaaImage { paatype, taggs, offsets: offs, palette, mipmaps };

		Ok(image)
	}


	/// Wrap `input` with a [`Cursor`][std::io::Cursor] and
	/// [`read_from`][`Self::read_from`] from it.
	pub fn from_bytes(input: &[u8]) -> PaaResult<Self> {
		let mut cursor = Cursor::new(input);
		Self::read_from(&mut cursor)
	}


	/// Convert self to PAA data as `Vec<u8>`.
	///
	/// Ignores input Taggs::Offs and regenerates offsets based on actual mipmap
	/// data.
	pub fn as_bytes(&self) -> PaaResult<Vec<u8>> {
		let mut buf: Vec<u8> = Vec::with_capacity(10_000_000);

		buf.extend(self.paatype.to_bytes().unwrap());

		for ref t in self.taggs.iter() {
			if let Tagg::Offs { .. } = t {
				continue;
			}

			buf.extend(t.as_bytes());
		}

		let offs_length = Tagg::Offs { offsets: vec![] }.as_bytes().len() as u32;

		let palette_data = if let Some(p) = &self.palette {
			p.as_bytes()?
		}
		else {
			vec![0u8, 0]
		};

		let mipmaps_offset = buf.len() as u32 + offs_length + palette_data.len() as u32;

		let mipmap_blocks = self.mipmaps
			.iter()
			.enumerate()
			.map(|(i, m)| {
				let m = m.clone().map_err(|e| InputMipmapErrorWhileEncoding(i, Box::new(e)))?;
				m.as_bytes().map_err(|e| MipmapErrorWhileSerializing(Box::new(e)))
			})
			.collect::<PaaResult<Vec<Vec<u8>>>>()?;

		let mipmap_block_offsets: Vec<u32> = mipmap_blocks
			.iter()
			.scan(0, |acc, b| {
				let current = *acc;
				let offset = current + mipmaps_offset;
				debug_trace!("mipmap_block_offsets: current={} b.len()={} offset={}", *acc, b.len(), offset);
				*acc += b.len() as u32;
				Some(offset)
			})
			.collect::<Vec<u32>>();

		let new_offs = Tagg::Offs { offsets: mipmap_block_offsets };
		buf.extend(new_offs.as_bytes());

		buf.extend(palette_data);

		for m in mipmap_blocks {
			buf.extend(m);
		}

		buf.extend([0u8; 6]);

		Ok(buf)
	}
}


#[derive(Debug, Clone, Copy, PartialEq, DekuRead, DekuWrite)]
#[cfg_attr(feature = "fuzz", derive(Arbitrary))]
#[deku(type = "u16", endian = "little")]
pub enum PaaType {
	// See `int __stdcall sub_4276E0(void *Block, int)` (ImageToPAA v1.0.0.3).
	#[deku(id = "0xFF_01")]
	Dxt1,

	#[deprecated]
	#[deku(id = "0xFF_02")]
	Dxt2,

	#[deprecated]
	#[deku(id = "0xFF_03")]
	Dxt3,

	#[deprecated]
	#[deku(id = "0xFF_04")]
	Dxt4,

	#[deku(id = "0xFF_05")]
	Dxt5,

	/// RGBA 4:4:4:4
	#[deku(id = "0x44_44")]
	Argb4444,

	/// RGBA 5:5:5:1
	#[deku(id = "0x15_55")]
	Argb1555,

	/// RGBA 8:8:8:8
	#[deku(id = "0x88_88")]
	Argb8888,

	/// 8 bits alpha, 8 bits grayscale
	#[deku(id = "0x80_80")]
	Ai88,

	/// 1 byte (offset into the index palette, which contains BGR 8:8:8)
	#[deprecated = "[TODO] Index palette format is not implemented"]
	#[deku(id = "0x47_47")]
	IndexPalette,
}


impl Default for PaaType {
	fn default() -> Self {
		PaaType::Dxt5
	}
}


impl PaaType {
	/// Calculates the size of uncompressed mipmap data from its width and
	/// height.
	pub const fn predict_size(&self, width: u16, height: u16) -> usize {
		use PaaType::*;

		const_assert!(std::mem::size_of::<usize>() >= 4);

		let mut result = width as usize * height as usize;

		match self {
			Dxt1 => { result /= 2 },
			IndexPalette | Dxt2 | Dxt3 | Dxt4 | Dxt5 => (),
			Argb4444 | Argb1555 | Ai88 => { result *= 2 },
			Argb8888 => { result *= 4 },
		}

		result
	}


	pub const fn is_dxtn(&self) -> bool {
		use PaaType::*;
		matches!(self, Dxt1 | Dxt2 | Dxt3 | Dxt4 | Dxt5)
	}
}


/// Metadata frame present in PAA headers.
#[derive(Debug, Display, Clone, PartialEq)]
pub enum Tagg {
	/// Average color value
	#[display(fmt = "Avgc {{ {} }}", rgba)]
	Avgc {
		rgba: Bgra8888Pixel,
	},

	/// Maximum color value
	#[display(fmt = "Maxc {{ {} }}", rgba)]
	Maxc {
		rgba: Bgra8888Pixel,
	},

	#[display(fmt = "Flag {{ {} }}", transparency)]
	Flag {
		/// Texture transparency type
		transparency: Transparency
	},

	/// Texture swizzle data (unknown format)
	#[display(fmt = "Swiz {{ {} }}", swizzle)]
	Swiz {
		swizzle: ArgbSwizzle,
	},

	/// Unknown metadata
	#[display(fmt = "{:?}", self)]
	Proc {
		code: TextureMacro,
	},

	/// Mipmap offsets
	#[display(fmt = "{:?}", self)]
	Offs {
		offsets: Vec<u32>
	},
}


impl Tagg {
	/// Serialize a Tagg into PAA-ready data.
	pub fn as_bytes(&self) -> Vec<u8> {
		const U32_SIZE: u32 = std::mem::size_of::<u32>() as u32;

		let mut bytes: Vec<u8> = Vec::with_capacity(256);
		bytes.extend("GGAT".as_bytes());
		bytes.extend(self.as_taggname().as_bytes());

		match self {
			Self::Avgc { rgba } => {
				extend_with_uint::<LittleEndian,Vec<u8>, _, 4>(&mut bytes, U32_SIZE);
				bytes.extend(rgba.to_bytes().unwrap());
			},

			Self::Maxc { rgba } => {
				extend_with_uint::<LittleEndian,Vec<u8>, _, 4>(&mut bytes, U32_SIZE);
				bytes.extend(rgba.to_bytes().unwrap());
			},

			Self::Flag { transparency } => {
				extend_with_uint::<LittleEndian,Vec<u8>, _, 4>(&mut bytes, U32_SIZE);
				bytes.extend(transparency.to_bytes().unwrap());
				bytes.extend([0x00u8, 0, 0]);
			},

			Self::Swiz { swizzle } => {
				extend_with_uint::<LittleEndian,Vec<u8>, _, 4>(&mut bytes, U32_SIZE);
				bytes.extend(swizzle.to_bytes().unwrap())
			},

			Self::Proc { code } => {
				let len = (code.text[..]).len() as u32;
				extend_with_uint::<LittleEndian,Vec<u8>, _, 4>(&mut bytes, len);
				bytes.extend(&code.text[..]);
			},

			Self::Offs { offsets } => {
				let len = (16 * std::mem::size_of::<u32>()) as u32;
				extend_with_uint::<LittleEndian,Vec<u8>, _, 4>(&mut bytes, len);

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


	/// Validate Tagg metadata contained in `data`: "TAGG" signature, tag name,
	/// and payload length.  Returns PaaResult<(name: String, payload_size: u32)>.
	pub fn try_head_from(data: &[u8; 12]) -> PaaResult<(String, u32)> {
		let taggsig = &data[0..4];

		// "GGAT" signature
		if taggsig != [0x47u8, 0x47, 0x41, 0x54] {
			return Err(UnexpectedTaggSignature);
		}

		let taggname: String = std::str::from_utf8(&data[4..8])
			.map_err(|_| UnknownTaggType((data[4..8]).try_into().unwrap()))?
			.into();

		if ! Self::is_valid_taggname(&taggname) {
			return Err(UnknownTaggType(taggname.as_bytes().try_into().unwrap()));
		}

		let payload_length = LittleEndian::read_u32(&data[8..12]);

		Ok((taggname, payload_length))
	}


	/// Constructs a [`Tagg`] from its name (e.g. "OFFS") and payload.
	pub fn from_name_and_payload(taggname: &str, data: &[u8]) -> PaaResult<Self> {
		if taggname.len() != 4 {
			return Err(UnexpectedTaggSignature);
		}

		match taggname {
			"CGVA" => {
				if data.len() != 4 {
					return Err(UnexpectedTaggDataSize);
				}
				let (_, rgba) = Bgra8888Pixel::from_bytes((data, 0)).unwrap();
				Ok(Self::Avgc { rgba })
			},

			"CXAM" => {
				if data.len() != 4 {
					return Err(UnexpectedTaggDataSize);
				}
				let (_, rgba) = Bgra8888Pixel::from_bytes((data, 0)).unwrap();
				Ok(Self::Maxc { rgba })
			},

			"GALF" => {
				if data.len() != 4 {
					return Err(UnexpectedTaggDataSize);
				}

				let (_, transparency) = Transparency::from_bytes((&data[0..1], 0))
					.map_err(|_| UnknownTransparencyValue(data[0]))?;

				Ok(Self::Flag { transparency })
			},

			"ZIWS" => {
				if data.len() != 4 {
					return Err(UnexpectedTaggDataSize);
				}
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


#[cfg(feature = "fuzz")]
impl<'a> Arbitrary<'a> for Tagg {
	fn arbitrary(input: &mut Unstructured) -> ArbitraryResult<Self> {
		use Tagg::*;

		let variant: usize = input.int_in_range(1..=6)?;

		let result = match variant {
			1 => {
				Avgc { rgba: input.arbitrary()? }
			},

			2 => {
				Maxc { rgba: input.arbitrary()? }
			},

			3 => {
				Flag { transparency: input.arbitrary()? }
			},

			4 => {
				Swiz { swizzle: input.arbitrary()? }
			},

			5 => {
				Proc { code: input.arbitrary()? }
			},

			6 => {
				let offs_len: usize = input.int_in_range(0..=16)?;
				let mut offsets: Vec<u32> = vec![0u32; offs_len];

				for o in offsets.iter_mut() {
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


#[derive(Default, Debug, Clone)]
pub struct PaaPalette {
	pub triplets: Vec<[u8; 3]>,
}


impl PaaPalette {
	/// Convert self to PAA data.
	pub fn as_bytes(&self) -> PaaResult<Vec<u8>> {
		const_assert!(std::mem::size_of::<usize>() >= std::mem::size_of::<u16>());

		if self.triplets.len() > u16::MAX as usize {
			return Err(PaletteTooLarge);
		}

		let ntriplets = self.triplets.len() as u16;
		let mut buf: Vec<u8> = Vec::with_capacity(2 + (ntriplets as usize) * 3);

		extend_with_uint::<LittleEndian, _, _, 2>(&mut buf, ntriplets);

		for triplet in self.triplets.iter() {
			buf.extend(triplet);
		}

		Ok(buf)
	}


	/// Returns `Ok(None)` if palette is empty, `Ok(palette)` otherwise.
	pub fn read_from<R: Read>(input: &mut R) -> PaaResult<Option<Self>> {
		const_assert!(std::mem::size_of::<usize>() >= 2);

		let len = input.read_u16::<LittleEndian>()? as usize;
		let mut triplets: Vec<[u8; 3]> = Vec::with_capacity(len);

		if len == 0 {
			return Ok(None);
		};

		for i in 0..len {
			let buf: [u8; 3] = read_exact_buffered(input, 3)?.try_into().expect("Could not convert buf (this is a bug)");
			triplets.insert(i, buf);
		};

		Ok(Some(Self { triplets }))
	}
}


#[derive(Debug, Clone, PartialEq)]
pub struct PaaMipmap {
	pub width: u16,
	pub height: u16,
	pub paatype: PaaType,
	pub compression: PaaMipmapCompression,
	pub data: Vec<u8>,
}


impl PaaMipmap {
	pub fn read_from<R: Read + Seek>(input: &mut R, paatype: PaaType) -> PaaResult<Self> {
		use PaaType::*;
		use PaaMipmapCompression::*;

		let pos = input.stream_position().unwrap();

		let mut paatype = paatype;
		let mut compression = PaaMipmapCompression::Uncompressed;

		let mut width = input.read_u16::<LittleEndian>()?;
		let mut height = input.read_u16::<LittleEndian>()?;

		if width == 0 || height == 0 {
			return Err(EmptyMipmap);
		}

		if width == 1234 && height == 8765 {
			paatype = PaaType::IndexPalette;
			compression = PaaMipmapCompression::Lzss;

			width = input.read_u16::<LittleEndian>()?;
			height = input.read_u16::<LittleEndian>()?;
		}

		if width & 0x8000 != 0 && paatype.is_dxtn() {
			compression = PaaMipmapCompression::Lzo;
			width ^= 0x8000;
		}

		const_assert!(std::mem::size_of::<usize>() >= 3);
		let data_len = paatype.predict_size(width, height);
		let data_compressed_len = input.read_uint::<LittleEndian>(3)? as usize;

		if matches!(paatype, IndexPalette) && !matches!(compression, Lzss) {
			compression = RleBlocks;
		}
		else if matches!(compression, Uncompressed) && data_len != data_compressed_len && !paatype.is_dxtn() {
			compression = Lzss;
		}

		let compressed_data_buf: Vec<u8> = read_exact_buffered(input, data_compressed_len)?;

		let data: Vec<u8> = match compression {
			Uncompressed => {
				compressed_data_buf
			},

			Lzo => {
				decompress_lzo_slice(&compressed_data_buf[..], data_len)?
			},

			Lzss => {
				let split_pos = compressed_data_buf.len().checked_sub(4).ok_or(CorruptedData)?;
				let (lzss_slice, checksum_slice) = compressed_data_buf.split_at(split_pos);
				let checksum = LittleEndian::read_i32(checksum_slice);
				let uncompressed_data = LzssReader::new().filter_slice_to_vec(lzss_slice).unwrap();

				if uncompressed_data.len() != data_len {
					return Err(LzssDecompressError);
				};

				let calculated_checksum = get_additive_i32_cksum(&uncompressed_data);

				if calculated_checksum != checksum {
					// [FIXME] keeps firing
					//debug_trace!("calculated_checksum != checksum: 0x{:08X} vs 0x{:08X}", calculated_checksum, checksum);
					//return Err(LzssWrongChecksum);
				}

				uncompressed_data
			},

			RleBlocks => {
				RleReader::new().filter_slice_to_vec(&compressed_data_buf[..]).map_err(RleError)?
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
			let is_eof = matches!(mip, Err(MipmapDataBeyondEof) | Err(EmptyMipmap) | Err(UnexpectedEof));

			result.push(mip);

			if is_eof {
				break;
			}
		}

		result
	}


	pub fn as_bytes(&self) -> PaaResult<Vec<u8>> {
		use PaaType::*;
		use PaaMipmapCompression::*;

		let mut bytes: SegVec<u8> = SegVec::new();

		if self.width >= 32768 || self.height >= 32768 {
			return Err(MipmapTooLarge);
		}

		let non_power_of_2 = self.width.count_ones() > 1 || self.height.count_ones() > 1;
		let too_small = self.width < 4 || self.height < 4;

		if self.paatype.is_dxtn() && (non_power_of_2 || too_small) {
			return Err(UnexpectedMipmapDimensions);
		}

		let mut width = self.width;
		let mut height = self.height;

	   if self.paatype.predict_size(width, height) != self.data.len() {
		   return Err(UnexpectedMipmapDataSize(width, height, self.data.len()));
	   }

		if let (Lzss, IndexPalette) = (&self.compression, &self.paatype) {
			if !self.is_empty() {
				width = 1234;
				height = 8765;
			}
		}

		if let Lzo = &self.compression {
			if self.paatype.is_dxtn() && !self.is_empty() {
				width ^= 0x8000;
			}
		}

		extend_with_uint::<LittleEndian, _, _, 2>(&mut bytes, width);
		extend_with_uint::<LittleEndian, _, _, 2>(&mut bytes, height);

		debug_trace!("MipMap::as_bytes: after width,height @ {}", bytes.len());

		if self.is_empty() {
			return Ok(bytes.into_iter().collect::<Vec<u8>>());
		}

		if let (Lzss { .. }, IndexPalette) = (&self.compression, &self.paatype) {
			extend_with_uint::<LittleEndian, _, _, 2>(&mut bytes, self.width);
			extend_with_uint::<LittleEndian, _, _, 2>(&mut bytes, self.height);

			// [TODO] Does the mipmap code on Biki mean that index palette lzss
			// data does not have `byte size[3]`?  I'm thinking probably not but
			// this needs to be tested on old PACs
		}

		debug_trace!("MipMap::as_bytes: after Lzss @ {}", bytes.len());

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
				let lzss_data = LzssWriter::new()
					.filter_slice_to_vec(&self.data[..])
					.unwrap();
				compressed_data.extend(lzss_data);

				let cksum = get_additive_i32_cksum(&self.data[..]);
				let mut buf = [0u8; 4];
				LittleEndian::write_i32(&mut buf, cksum);
				compressed_data.extend(buf);
			},

			RleBlocks => {
				let rle_data = RleWriter::with_minimum_run(3)
					.filter_slice_to_vec(&self.data[..])
					.unwrap();
				compressed_data.extend(rle_data);
			},
		}

		extend_with_uint::<LittleEndian, _, u32, 3>(&mut bytes, compressed_data.len() as u32);
		debug_trace!("MipMap::as_bytes: after length @ {}", bytes.len());
		bytes.extend(&compressed_data[..]);
		debug_trace!("MipMap::as_bytes: after data @ {}", bytes.len());

		Ok(bytes.into_iter().collect::<Vec<u8>>())
	}


	pub fn is_empty(&self) -> bool {
		self.width == 0 || self.height == 0
	}
}


#[cfg(feature = "fuzz")]
impl <'a> Arbitrary<'a> for PaaMipmap {
	fn arbitrary(input: &mut Unstructured) -> ArbitraryResult<Self> {
		use PaaType::*;
		use PaaMipmapCompression::*;

		let paatype = <PaaType as Arbitrary>::arbitrary(input)?;

		let compression = match &paatype {
			Dxt1 | Dxt2 | Dxt3 | Dxt4 | Dxt5 => Lzo,
			IndexPalette => *input.choose(&[Lzss, RleBlocks])?,
			_ => <PaaMipmapCompression as Arbitrary>::arbitrary(input)?,
		};

		let (width, height) = if paatype.is_dxtn() {
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

		let data_len = paatype.predict_size(width, height);
		let mut data: Vec<u8> = vec![0u8; data_len];
		input.fill_buffer(&mut data)?;

		Ok(Self { width, height, paatype, compression, data })
	}
}


/// The color data used in AVGCTAGG and MAXCTAGG; its byte layout is B:G:R:A.
#[derive(Debug, Clone, Copy, PartialEq, DekuRead, DekuWrite)]
#[cfg_attr(feature = "fuzz", derive(Arbitrary))]
pub struct Bgra8888Pixel {
	b: u8,
	g: u8,
	r: u8,
	a: u8,
}


impl std::fmt::Display for Bgra8888Pixel {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(f, "<r={:.3}> <g={:.3}> <b={:.3}> <a={:.3}>",
			self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0, self.a as f32 / 255.0)
	}
}


#[derive(Debug, Display, Clone, PartialEq, DekuRead, DekuWrite)]
#[cfg_attr(feature = "fuzz", derive(Arbitrary))]
#[deku(type = "u8")]
pub enum Transparency {
	#[display(fmt = "<no transparency>")]
	#[deku(id = "0x00")]
	None,
	#[display(fmt = "<transparent, interpolated alpha>")]
	#[deku(id = "0x01")]
	AlphaInterpolated,
	#[display(fmt = "<transparent, non-interpolated alpha>")]
	#[deku(id = "0x02")]
	AlphaNotInterpolated,
}


impl Default for Transparency {
	fn default() -> Self {
		Transparency::AlphaInterpolated
	}
}


#[derive(Debug, Display, Clone, Copy, PartialEq, DekuRead, DekuWrite)]
#[display(fmt = "{}, {}, {}, {}", a, r, g, b)]
pub struct ArgbSwizzle {
	#[deku(ctx = "ChannelSwizzleId::Alpha")]
	a: ChannelSwizzle,
	#[deku(ctx = "ChannelSwizzleId::Red")]
	r: ChannelSwizzle,
	#[deku(ctx = "ChannelSwizzleId::Green")]
	g: ChannelSwizzle,
	#[deku(ctx = "ChannelSwizzleId::Blue")]
	b: ChannelSwizzle,
}


impl ArgbSwizzle {
	pub fn as_rgba8_filter(&self) -> Box<dyn FnMut(&[u8; 4]) -> [u8; 4]> {
		let mut a_flt = self.a.as_subpixel_map();
		let mut r_flt = self.r.as_subpixel_map();
		let mut g_flt = self.g.as_subpixel_map();
		let mut b_flt = self.b.as_subpixel_map();

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


#[derive(Debug, Display, Clone, Copy, PartialEq, DekuRead, DekuWrite)]
#[cfg_attr(feature = "fuzz", derive(Arbitrary))]
#[deku(ctx = "tgt: ChannelSwizzleId")]
#[display(fmt = "<{}={}>", target, data)]
pub struct ChannelSwizzle {
	#[deku(skip, default = "tgt")]
	pub target: ChannelSwizzleId,
	#[deku(pad_bits_before = "4")]
	pub data: ChannelSwizzleData,
}


impl ChannelSwizzle {
	pub fn as_subpixel_map(&self) -> Box<dyn FnMut(&[u8; 4], &mut [u8; 4])> {
		use ChannelSwizzleData::*;

		let target_idx = self.target.as_rgba_index();

		match self.data {
			Source { neg_flag: false, source } => {
				let source_idx = source.as_rgba_index();
				Box::new(move |src: &[u8; 4], dst: &mut [u8; 4]| { dst[target_idx] = src[source_idx] })
			},

			Source { neg_flag: true, source } => {
				let source_idx = source.as_rgba_index();
				Box::new(move |src: &[u8; 4], dst: &mut [u8; 4]| { dst[target_idx] = 0xFF - src[source_idx] })
			},

			Fill { value } => {
				let fill_byte: u8 = value as u8;

				Box::new(move |_: &[u8; 4], dst: &mut [u8; 4]| { dst[target_idx] = fill_byte })
			},
		}
	}
}


#[derive(Debug, Display, Clone, Copy, PartialEq, DekuRead, DekuWrite)]
#[cfg_attr(feature = "fuzz", derive(Arbitrary))]
#[deku(type = "u8", bits = "2")]
pub enum ChannelSwizzleId {
	#[display(fmt = "a")]
	#[deku(id = "0b00")]
	Alpha,
	#[display(fmt = "r")]
	#[deku(id = "0b01")]
	Red,
	#[display(fmt = "g")]
	#[deku(id = "0b10")]
	Green,
	#[display(fmt = "b")]
	#[deku(id = "0b11")]
	Blue,
}


impl ChannelSwizzleId {
	fn as_rgba_index(&self) -> usize {
		use ChannelSwizzleId::*;

		match self {
			Red => 0,
			Green => 1,
			Blue => 2,
			Alpha => 3,
		}
	}
}


#[derive(Debug, Clone, Copy, PartialEq, DekuRead, DekuWrite)]
#[deku(type = "u8", bits = "1")]
pub enum ChannelSwizzleData {
	#[deku(id = "0b0")]
	Source {
		#[deku(bits = "1")]
		neg_flag: bool,
		source: ChannelSwizzleId,
	},

	#[deku(id = "0b1")]
	Fill {
		#[deku(pad_bits_before = "1")]
		value: ChannelSwizzleFill
	},
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


#[cfg(feature = "fuzz")]
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


#[derive(Debug, Display, Clone, Copy, PartialEq, DekuRead, DekuWrite)]
#[cfg_attr(feature = "fuzz", derive(Arbitrary))]
#[deku(type = "u8", bits = "2")]
#[repr(u8)]
pub enum ChannelSwizzleFill {
	#[display(fmt = "1")]
	#[deku(id = "0b00")]
	FillFF = 0xFF,
	#[display(fmt = "0")]
	#[deku(id = "0b01")]
	Fill00 = 0x00,
}


#[derive(Debug, Display, Clone, PartialEq)]
pub struct TextureMacro {
	pub text: BString,
}


#[cfg(feature = "fuzz")]
impl<'a> Arbitrary<'a> for TextureMacro {
	fn arbitrary(input: &mut Unstructured) -> ArbitraryResult<Self> {
		Ok(TextureMacro { text: BString::from(<Vec<u8> as Arbitrary>::arbitrary(input)?) })
	}
}


/// The algorithm compressing the data of a given mipmap.
#[derive(Debug, Copy, Clone, PartialEq)]
#[cfg_attr(feature = "fuzz", derive(Arbitrary))]
pub enum PaaMipmapCompression {
	Uncompressed,

	Lzo,

	Lzss,

	RleBlocks,
}


pub struct PaaDecoder {
	paa: PaaImage,
}


impl PaaDecoder {
	pub fn from_paa(paa: PaaImage) -> Self {
		Self { paa }
	}


	pub fn decode_nth(&self, index: usize) -> PaaResult<RgbaImage> {
		let mipmap = self.paa.mipmaps
			.get(index)
			.ok_or(MipmapIndexOutOfRange)?
			.as_ref()
			.map_err(|e| e.clone())?;

		decode_mipmap(mipmap)
	}


	pub fn decode_first(&self) -> PaaResult<RgbaImage> {
		self.decode_nth(0)
	}
}


/// A convenience function which extends an [`std::iter::Extend<u8>`] with a
/// [`byteorder::ByteOrder`]-encoded integer.
fn extend_with_uint<B, E, T, const N: usize>(e: &mut E, v: T)
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

	extend_with_uint::<LittleEndian, _, _, 2>(&mut dest, 1234u16);
	assert_eq!(dest, vec![0xD2, 0x04]);

	extend_with_uint::<LittleEndian, _, _, 3>(&mut dest, 1234u32);
	assert_eq!(dest, vec![0xD2, 0x04, 0xD2, 0x04, 0x00]);

	extend_with_uint::<BigEndian, _, _, 4>(&mut dest, 5678u32);
	assert_eq!(dest, vec![0xD2, 0x04, 0xD2, 0x04, 0x00, 0x00, 0x00, 0x16, 0x2E]);
}


fn read_exact_buffered<R: Read>(input: &mut R, len: usize) -> PaaResult<Vec<u8>> {
	const SINGLE_READ_SIZE: usize = 64;
	let mut data: SegVec<u8> = SegVec::new();
	let mut total = 0usize;

	loop {
		if total == len {
			break;
		};

		let bufsize = std::cmp::min(SINGLE_READ_SIZE, len-total);
		let mut buf = vec![0u8; bufsize];
		input.read_exact(&mut buf)?;
		data.extend(&buf[..]);
		total += bufsize;
	}

	Ok(data.into_iter().collect::<Vec<u8>>())
}


#[test]
fn test_read_exact_buffered() {
	let mut input = Cursor::new(vec![0x41u8, 0x42, 0x43, 0x44, 0x45, 0x46]);
	assert_eq!(read_exact_buffered(&mut input, 1).unwrap(), vec![0x41u8]);
	assert_eq!(read_exact_buffered(&mut input, 2).unwrap(), vec![0x42u8, 0x43]);
	assert_eq!(read_exact_buffered(&mut input, 3).unwrap(), vec![0x44u8, 0x45, 0x46]);
}


fn get_additive_i32_cksum(_: &[u8]) -> i32 {
	0
}


fn decompress_lzo_slice(input: &[u8], dst_len: usize) -> PaaResult<Vec<u8>> {
	let lzo = minilzo_rs::LZO::init().unwrap();
	lzo.decompress_safe(input, dst_len).map_err(|e| LzoError(format!("{:?}", e)))
}


fn compress_lzo_slice(input: &[u8]) -> PaaResult<Vec<u8>> {
	let mut lzo = minilzo_rs::LZO::init().unwrap();
	lzo.compress(input).map_err(|e| LzoError(format!("{:?}", e)))
}


fn decode_mipmap(mipmap: &PaaMipmap) -> PaaResult<RgbaImage> {
	use PaaType::*;

	if mipmap.is_empty() {
		return Err(EmptyMipmap);
	};

	match mipmap.paatype {
		paatype @ (Dxt1 | Dxt2 | Dxt3 | Dxt4 | Dxt5) => {
			let (comp_ratio, format) = match &paatype {
				Dxt1 => (8, SquishFormat::Bc1),
				Dxt2 => (4, SquishFormat::Bc2),
				Dxt3 => (4, SquishFormat::Bc2),
				Dxt4 => (4, SquishFormat::Bc3),
				Dxt5 => (4, SquishFormat::Bc3),
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


pub fn apply_swizzle_to_rgba8(swiz: &crate::ArgbSwizzle, rgba8: &mut image::RgbaImage) {
	let mut flt = swiz.as_rgba8_filter();

	for pixel in rgba8.pixels_mut() {
		let src = pixel.channels();
		let dst = flt(src.try_into().unwrap());
		pixel.channels_mut().copy_from_slice(&dst)
	};
}



pub(crate) fn argb4444_to_rgba8888(data4: &[u8]) -> Vec<u8> {
	assert_eq!(data4.len() % 2, 0, "Truncated ARGB4444 data in input");

	let mut result = Vec::with_capacity(data4.len()*2);

	for pixel in data4.chunks(2) {
		let pixel = LittleEndian::read_u16(pixel).to_be_bytes();
		let pixel = &pixel;

		let a: u8 = pixel[0] >> 4;
		let r: u8 = pixel[0] & 0x0F;
		let g: u8 = pixel[1] >> 4;
		let b: u8 = pixel[1] & 0x0F;

		let r: u8 = ((r as u16 * 0xFF + 0x1) / 0x0F) as u8;
		let g: u8 = ((g as u16 * 0xFF + 0x1) / 0x0F) as u8;
		let b: u8 = ((b as u16 * 0xFF + 0x1) / 0x0F) as u8;
		let a: u8 = ((a as u16 * 0xFF + 0x1) / 0x0F) as u8;

		result.extend([r, g, b, a]);
	};

	result
}


pub(crate) fn argb1555_to_rgba8888(data5: &[u8]) -> Vec<u8> {
	assert_eq!(data5.len() % 2, 0, "Truncated ARGB1555 data in input");

	let mut result = Vec::with_capacity(data5.len()*2);

	for pixel in data5.chunks(2) {
		let pixel = LittleEndian::read_u16(pixel).to_be_bytes();
		let pixel = &pixel;

		let a: u8 = pixel[0] >> 7;
		let r: u8 = (pixel[0] >> 2) & 0x1F;
		let g: u8 = (pixel[0] << 3 | pixel[1] >> 5) & 0x1F;
		let b: u8 = pixel[1] & 0x1F;

		let r: u8 = ((r as u16 * 0xFF + 0xF) / 0x1F) as u8;
		let g: u8 = ((g as u16 * 0xFF + 0xF) / 0x1F) as u8;
		let b: u8 = ((b as u16 * 0xFF + 0xF) / 0x1F) as u8;
		let a: u8 = a * 0xFF;

		result.extend([r, g, b, a]);
	};

	result
}


pub(crate) fn argb8888_to_rgba8888(data8: &[u8]) -> Vec<u8> {
	assert_eq!(data8.len() % 4, 0, "Truncated ARGB8888 data in input");

	let mut result = Vec::with_capacity(data8.len());

	for pixel in data8.chunks(4) {
		result.extend(pixel.iter().rev());
	};

	result
}
