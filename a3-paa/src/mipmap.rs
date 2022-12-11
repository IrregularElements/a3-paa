use std::fmt::Debug;
use std::io::{Read, Seek, SeekFrom, Cursor};
use std::iter::Extend;
use std::default::Default;

#[cfg(feature = "fuzz")] use arbitrary::{Arbitrary, Unstructured, Result as ArbitraryResult};
use byteorder::{LittleEndian, ByteOrder, ReadBytesExt};
use image::RgbaImage;
use texpresso::Format as TextureFormat;
use static_assertions::const_assert;
use surety::Ensure;
use bohemia_compression::*;


use crate::PaaResult;
use crate::PaaError::*;
use crate::PaaType;
use crate::get_additive_i32_cksum;
use crate::ReadExt;
use crate::ExtendExt;
use crate::pixel::*;
use crate::macros;
#[cfg(doc)] use crate::PaaImage;


/// A single mipmap (image) from a [`PaaImage`]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaaMipmap {
	/// Width in pixels.  Must not be larger than 32767; MSB indicates compression.
	pub width: u16,
	/// Height in pixels.
	pub height: u16,
	/// Underlying data type. Equals to the type of the parent [`PaaImage`].
	pub paatype: PaaType,
	/// Compression used when serializing this mipmap.
	pub compression: PaaMipmapCompression,
	/// Uncompressed [`paatype`][`Self::paatype`]-encoded image data.
	pub data: Vec<u8>,
}


impl PaaMipmap {
	/// Attempt to read the mipmap from a [`Read`].
	///
	/// # Errors
	/// - [`EmptyMipmap`]: Width or height of the mipmap is 0.
	/// - [`UnexpectedEof`]: [`std::io::Read::read_exact()`] or
	///   [`byteorder::ReadBytesExt::read_uint()`] encountered an EOF.
	/// - [`UnexpectedIoError`]: [`std::io::Read::read_exact()`] or
	///   [`byteorder::ReadBytesExt::read_uint()`] encountered an unexpected I/O error.
	/// - [`LzoError`]: Failed to decompress LZO data.
	/// - [`LzssDecompressError`]: LZSS data did not expand to the length
	///   computed by [`PaaType::predict_size`].
	/// - [`RleError`]: Failed to decompress RLE data.
	/// - [`ArithmeticOverflow`]: LZSS data did not have enough space for the
	///   checksum.
	///
	/// # Panics
	/// - If [`deku::DekuContainerWrite::to_bytes()`] fails (should never happen).
	/// - If [`bohemia_compression::LzssReader::filter_slice_to_vec()`] fails (should never happen).
	///
	/// [`Read`]: std::io::Read
	pub fn read_from<R: Read>(input: &mut R, paatype: PaaType) -> PaaResult<Self> {
		use PaaType::*;
		use PaaMipmapCompression::*;

		let mut paatype = paatype;
		let mut compression = Uncompressed;

		let mut width = input.read_u16::<LittleEndian>()?;
		let mut height = input.read_u16::<LittleEndian>()?;

		if width == 0 || height == 0 {
			return Err(EmptyMipmap);
		};

		if width == 1234 && height == 8765 {
			paatype = IndexPalette;
			compression = Lzss;

			width = input.read_u16::<LittleEndian>()?;
			height = input.read_u16::<LittleEndian>()?;
		};

		if width & 0x8000 != 0 && paatype.is_dxtn() {
			compression = Lzo;
			width ^= 0x8000;
		};

		const_assert!(std::mem::size_of::<usize>() >= 3);
		let data_len = paatype.predict_size(width, height);
		#[allow(clippy::cast_possible_truncation)]
		let data_compressed_len = input.read_uint::<LittleEndian>(3)? as usize;

		if matches!(paatype, IndexPalette) && !matches!(compression, Lzss) {
			compression = RleBlocks;
		}
		else if matches!(compression, Uncompressed) && data_len != data_compressed_len && !paatype.is_dxtn() {
			compression = Lzss;
		};

		let compressed_data_buf: Vec<u8> = input.read_exact_buffered(data_compressed_len)?;

		let data: Vec<u8> = match compression {
			Uncompressed => compressed_data_buf,

			Lzo => Lzo.decompress_slice(&compressed_data_buf[..], data_len)?,

			Lzss => {
				let split_pos = compressed_data_buf.len().checked_sub(4).ok_or(ArithmeticOverflow)?;
				let (lzss_slice, checksum_slice) = compressed_data_buf.split_at(split_pos);
				let checksum = LittleEndian::read_i32(checksum_slice);
				let uncompressed_data = LzssReader::new().filter_slice_to_vec(lzss_slice).unwrap();

				if uncompressed_data.len() != data_len {
					return Err(LzssDecompressError);
				};

				let calculated_checksum = get_additive_i32_cksum(&uncompressed_data);

				if calculated_checksum != checksum {
					// [FIXME] keeps firing
					//return Err(LzssWrongChecksum);
				};

				uncompressed_data
			},

			RleBlocks => RleReader::new().filter_slice_to_vec(&compressed_data_buf[..]).map_err(RleError)?,
		};

		Ok(PaaMipmap { width, height, paatype, compression, data })
	}


	/// Read a single mipmap from 'input'.
	///
	/// # Errors
	/// - [`EmptyMipmap`]: Width or height of the mipmap is 0.
	/// - [`UnexpectedEof`]: [`std::io::Read::read_exact()`] or
	///   [`byteorder::ReadBytesExt::read_uint()`] encountered an EOF.
	/// - [`LzoError`]: Failed to decompress LZO data.
	/// - [`LzssDecompressError`]: LZSS data did not expand to the length
	///   computed by [`PaaType::predict_size`].
	/// - [`RleError`]: Failed to decompress RLE data.
	/// - [`ArithmeticOverflow`]: LZSS data did not have enough space for the
	///   checksum.
	///
	/// # Panics
	/// - If [`deku::DekuContainerWrite::to_bytes()`] fails (should never happen).
	/// - If [`bohemia_compression::LzssReader::filter_slice_to_vec()`] fails (should never happen).
	///
	pub fn from_bytes(input: &[u8], paatype: PaaType) -> PaaResult<Self> {
		let mut cursor = Cursor::new(input);
		Self::read_from(&mut cursor, paatype)
	}


	/// Read sequential mipmaps from `input` until end of file.
	pub fn read_from_until_eof<R: Read>(input: &mut R, paatype: PaaType) -> Vec<PaaResult<Self>> {
		let mut result: Vec<PaaResult<PaaMipmap>> = Vec::with_capacity(8);

		loop {
			let mip = PaaMipmap::read_from(input, paatype);
			let is_eof = matches!(mip, Err(MipmapDataBeyondEof | EmptyMipmap | UnexpectedEof));

			result.push(mip);

			if is_eof {
				break;
			};
		};

		result
	}


	/// Read sequential mipmaps from `input` until end of file.
	pub fn read_from_with_offsets<R: Read + Seek>(input: &mut R, offsets: &[u32], paatype: PaaType) -> Vec<PaaResult<Self>> {
		let read_from_offset = |input: &mut R, offset: u32| -> PaaResult<Self> {
			let _ = input.seek(SeekFrom::Start(offset.into()))?;
			PaaMipmap::read_from(input, paatype)
		};

		offsets.iter().map(|o| read_from_offset(input, *o)).collect::<Vec<_>>()
	}


	/// # Errors
	/// - [`MipmapTooLarge`]: Mipmap dimension equals to or is larger than 32768.
	/// - [`UnexpectedMipmapDimensions`]: Attempted to encode a DXTn texture
	///   that is too small or not a power of 2.
	/// - [`UnexpectedMipmapDataSize`]: [`PaaMipmap::data.len()`] does not equal
	///   [`PaaType::predict_size`].
	///
	/// # Panics
	/// - If [`bohemia_compression::LzssWriter::filter_slice_to_vec()`] fails
	///   (should never happen).
	/// - If [`bohemia_compression::RleWriter::filter_slice_to_vec()`] fails
	///   (should never happen).
	pub fn to_bytes(&self) -> PaaResult<Vec<u8>> {
		use PaaType::*;
		use PaaMipmapCompression::*;

		let mut bytes: Vec<u8> = Vec::with_capacity(self.bytes_size_hint());

		if self.width >= 32768 || self.height >= 32768 {
			return Err(MipmapTooLarge);
		};

		let non_power_of_2 = self.width.count_ones() > 1 || self.height.count_ones() > 1;
		let too_small = self.width < 4 || self.height < 4;

		if self.paatype.is_dxtn() && (non_power_of_2 || too_small) {
			return Err(UnexpectedMipmapDimensions);
		};

		let mut width = self.width;
		let mut height = self.height;

		if self.paatype.predict_size(width, height) != self.data.len() {
			return Err(UnexpectedMipmapDataSize(width, height, self.data.len()));
		};

		if let (Lzss, IndexPalette) = (&self.compression, &self.paatype) {
			if !self.is_empty() {
				width = 1234;
				height = 8765;
			};
		};

		if let Lzo = &self.compression {
			if self.paatype.is_dxtn() && !self.is_empty() {
				width ^= 0x8000;
			};
		};

		bytes.extend_with_uint::<LittleEndian, _, 2>(width);
		bytes.extend_with_uint::<LittleEndian, _, 2>(height);

		if self.is_empty() {
			return Ok(bytes.into_iter().collect::<Vec<u8>>());
		};

		if let (Lzss { .. }, IndexPalette) = (&self.compression, &self.paatype) {
			bytes.extend_with_uint::<LittleEndian, _, 2>(self.width);
			bytes.extend_with_uint::<LittleEndian, _, 2>(self.height);

			// [TODO] Does the mipmap code on Biki mean that index palette lzss
			// data does not have `byte size[3]`?  I'm thinking probably not but
			// this needs to be tested on old PACs
		};

		let mut compressed_data: Vec<u8> = Vec::with_capacity(std::cmp::min(self.data.len() * 2, 128));

		let data = self.compression.compress_slice(&self.data[..])?;
		compressed_data.extend(data);

		if self.compression == PaaMipmapCompression::Lzss {
			let cksum = get_additive_i32_cksum(&self.data[..]);
			let mut buf = [0u8; 4];
			LittleEndian::write_i32(&mut buf, cksum);
			compressed_data.extend(buf);
		};

		const_assert!(std::mem::size_of::<usize>() >= 4);

		#[allow(clippy::cast_possible_truncation)]
		if compressed_data.len() > u32::MAX as usize {
			return Err(MipmapTooLarge);
		};

		#[allow(clippy::cast_possible_truncation)]
		bytes.extend_with_uint::<LittleEndian, u32, 3>(compressed_data.len() as u32);
		bytes.extend(&compressed_data[..]);

		Ok(bytes.into_iter().collect::<Vec<u8>>())
	}


	/// Return true if any dimension is 0.
	pub fn is_empty(&self) -> bool {
		self.width == 0 || self.height == 0
	}


	/// Returns `true` if a DXTn mipmap of size `w*h` needs LZO compression.
	pub fn dxtn_needs_lzo(width: u16, height: u16) -> bool {
		u32::from(width) * u32::from(height) >= 256 * 256
	}


	/// Returns the expected compression type for a mipmap of given `paatype`,
	/// `width` and `height`.
	pub fn suggest_compression(paatype: PaaType, width: u16, height: u16) -> PaaMipmapCompression {
		use PaaMipmapCompression::*;

		match paatype {
			c if c.is_dxtn() => if Self::dxtn_needs_lzo(width, height) { Lzo } else { Uncompressed },
			_ => Lzss,
		}
	}


	/// Attempt to decode `self` into an [`image::RgbaImage`].
	pub(crate) fn decode(&self) -> PaaResult<RgbaImage> {
		use PaaType::*;

		if self.is_empty() {
			return Err(EmptyMipmap);
		};

		match self.paatype {
			paatype if paatype.is_dxtn() => {
				#[allow(clippy::match_same_arms)]
				let (comp_ratio, format) = match &paatype {
					Dxt1 => (8, TextureFormat::Bc1),
					Dxt2 => (4, TextureFormat::Bc2),
					Dxt3 => (4, TextureFormat::Bc2),
					Dxt4 => (4, TextureFormat::Bc3),
					Dxt5 => (4, TextureFormat::Bc3),
					_ => unreachable!(),
				};

				let buf_len = self.data.len()
					.checked_mul(comp_ratio)
					.ok_or(MipmapTooLarge)?;
				let mut buffer = vec![0u8; buf_len];
				format.decompress(&self.data, self.width.into(), self.height.into(), &mut buffer);
				let image = RgbaImage::from_vec(self.width.into(), self.height.into(), buffer).unwrap();
				Ok(image)
			},

			Argb4444 => {
				let data = Argb4444Pixel::convert_to_rgba8_slice(&self.data)?;
				let image = RgbaImage::from_vec(self.width.into(), self.height.into(), data).unwrap();
				Ok(image)
			},

			Argb1555 => {
				let data = Argb1555Pixel::convert_to_rgba8_slice(&self.data)?;
				let image = RgbaImage::from_vec(self.width.into(), self.height.into(), data).unwrap();
				Ok(image)
			},

			f => todo!("Pixel format not yet implemented: {:?}", f),
		}
	}


	pub(crate) fn encode(paatype: PaaType, image: &image::RgbaImage) -> PaaResult<Self> {
		use PaaType::*;

		let (w, h) = image.dimensions();
		let width: u16 = w.try_into().map_err(|_| MipmapTooLarge)?;
		let height: u16 = h.try_into().map_err(|_| MipmapTooLarge)?;
		let compression = PaaMipmap::suggest_compression(paatype, width, height);

		match paatype {
			t if t.is_dxtn() => {
				let textureformat = match t {
					Dxt1 => TextureFormat::Bc1,
					Dxt2 | Dxt3 => TextureFormat::Bc2,
					Dxt4 | Dxt5 => TextureFormat::Bc3,
					_ => unreachable!(),
				};

				let mut data: Vec<u8> = vec![0; textureformat.compressed_size(width.into(), height.into())];
				let params = texpresso::Params { algorithm: texpresso::Algorithm::IterativeClusterFit, ..Default::default() };
				textureformat.compress(image.as_raw(), width.into(), height.into(), params, &mut data);
				let mipmap = PaaMipmap { width, height, paatype, compression, data };
				Ok(mipmap)
			},

			Argb1555 => {
				let data = Argb1555Pixel::convert_from_rgba8_slice(image.as_raw())?;
				let mipmap = PaaMipmap { width, height, paatype, compression, data };
				Ok(mipmap)
			},

			Argb4444 => {
				let data = Argb4444Pixel::convert_from_rgba8_slice(image.as_raw())?;
				let mipmap = PaaMipmap { width, height, paatype, compression, data };
				Ok(mipmap)
			},

			t => todo!("PaaMipmap::encode: PaaType not yet implemented: {:?}", t),
		}
	}


	fn bytes_size_hint(&self) -> usize {
		// [TODO]
		let result = 0usize.checked();
		result.unwrap_or(10_000_000)
	}
}


impl Default for PaaMipmap {
	fn default() -> Self {
		let width = 0;
		let height = 0;
		let paatype = PaaType::Dxt5;
		let compression = PaaMipmap::suggest_compression(paatype, width, height);
		let data = vec![];
		PaaMipmap { width, height, paatype, compression, data }
	}
}


#[cfg(feature = "fuzz")]
impl<'a> Arbitrary<'a> for PaaMipmap {
	fn arbitrary(input: &mut Unstructured) -> ArbitraryResult<Self> {
		use PaaType::*;
		use PaaMipmapCompression::*;

		let variant: usize = input.int_in_range(1..=4)?;

		let (paatype, compression, width, height) = match variant {
			1 => {
				// PaaType = DXTn, conditional LZO compression
				let paatype = *input.choose(&[Dxt1, Dxt2, Dxt3, Dxt4, Dxt5])?;
				let width: u16 = 2u16.pow(input.int_in_range(2..=10)?);
				let height: u16 = 2u16.pow(input.int_in_range(2..=10)?);

				let compression = PaaMipmap::suggest_compression(paatype, width, height);

				(paatype, compression, width, height)
			},

			variant @ (2 | 3) => {
				// PaaType = IndexPalette, LZSS or RLE compression
				let width: u16 = input.int_in_range(1..=2000)?;
				let height: u16 = input.int_in_range(1..=2000)?;

				let compression = if variant == 2 { Lzss } else { RleBlocks };

				(IndexPalette, compression, width, height)
			},

			4 => {
				// PaaType = other, LZSS compression
				let paatype = *input.choose(&[Argb1555, Argb4444, Argb8888, Ai88])?;

				let width: u16 = input.int_in_range(1..=2000)?;
				let height: u16 = input.int_in_range(1..=2000)?;

				let compression = PaaMipmap::suggest_compression(paatype, width, height);

				(paatype, compression, width, height)
			},

			_ => unreachable!(),
		};

		let data_len = paatype.predict_size(width, height);
		let mut data = vec![0u8; data_len];
		input.fill_buffer(&mut data)?;

		Ok(Self { width, height, paatype, compression, data })
	}
}


/// The algorithm compressing the data of a given mipmap
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "fuzz", derive(Arbitrary))]
pub enum PaaMipmapCompression {
	/// Data is stored as-is.
	Uncompressed,
	/// LZO compression (for DXTn textures).
	Lzo,
	/// LZSS (F=16) (for RGB and legacy index palette textures).
	Lzss,
	/// RLE-based compression similar to TGA's PackBits (for legacy index
	/// palette textures).
	RleBlocks,
}


impl PaaMipmapCompression {
	/// # Errors
	/// - [`LzoError`]: failed to compress input as LZO.
	/// - [`RleError`]: `RleReader` failed to compress `input` as RLE.
	///
	/// # Panics
	/// - If `LzssWriter` fails to compress `input`.
	#[allow(clippy::missing_panics_doc)]
	pub fn compress_slice(self, input: &[u8]) -> PaaResult<Vec<u8>> {
		use PaaMipmapCompression::*;
		match self {
			Uncompressed => Ok(input.to_vec()),
			Lzo => {
				let mut lzo = minilzo_rs::LZO::init().unwrap();
				lzo.compress(input).map_err(|e| LzoError(format!("{:?}", e)))
			},
			Lzss => {
				macros::log!(trace, "LZSS compression");
				let data = LzssWriter::new().filter_slice_to_vec(input).unwrap();
				Ok(data)
			},
			RleBlocks => RleWriter::new().filter_slice_to_vec(input).map_err(RleError),
		}
	}


	/// # Errors
	/// - [`LzoError`]: failed to decompress input as LZO.
	/// - [`LzssDecompressError`]: `LzssReader` failed to decompress `input` as LZSS.
	/// - [`RleError`]: `RleReader` failed to decompress `input` as RLE.
	#[allow(clippy::missing_panics_doc)]
	pub fn decompress_slice(self, input: &[u8], dst_len: usize) -> PaaResult<Vec<u8>> {
		use PaaMipmapCompression::*;
		match self {
			Uncompressed => Ok(input.to_vec()),
			Lzo => {
				let lzo = minilzo_rs::LZO::init().unwrap();
				lzo.decompress_safe(input, dst_len).map_err(|e| LzoError(format!("{:?}", e)))
			},
			Lzss => LzssReader::new().filter_slice_to_vec(input).map_err(|_| LzssDecompressError),
			RleBlocks => RleReader::new().filter_slice_to_vec(input).map_err(RleError),
		}
	}
}
