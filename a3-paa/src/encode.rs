use crate::macros;
use crate::imageops;
use crate::cfgfile;

use crate::{PaaResult, PaaType, PaaImage, Tagg, PaaMipmap, ArgbSwizzle};
#[cfg(doc)] use crate::PaaError::*;

use std::collections::HashMap;
use std::ops::Deref;

use enum_utils::FromStr;
use image::RgbaImage;


/// Wrapper around [`TextureEncodingSettings`] that encodes an
/// [`image::RgbaImage`] into a [`PaaImage`]
///
/// [`RgbaImage`]: [image::RgbaImage]
#[allow(missing_debug_implementations)]
#[derive(Clone)]
pub struct PaaEncoder {
	image: RgbaImage,
	settings: TextureEncodingSettings,
}


impl PaaEncoder {
	/// Creates a new encoder from an [`image::RgbaImage`] and
	/// [`TextureEncodingSettings`].
	pub fn with_image_and_settings(image: RgbaImage, settings: TextureEncodingSettings) -> Self {
		Self { image, settings }
	}


	/// # Panics
	/// - If `self.image.width * self.image.height` overflows a [`u64`].
	#[allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]
	pub fn encode(&self) -> PaaResult<PaaImage> {
		use image::GenericImageView;

		let mut img = self.image.clone();

		// [TODO] It would seem that AVGC and MAXC are computed from the texture
		// *before* swizzling, although this needs testing.
		let (mut avgc, mut maxc) = imageops::get_avgc_maxc(&img);

		self.settings.swizzle.apply_to_image(&mut img);

		if self.settings.autoreduce && imageops::is_solid_color(&img) {
			img = img.view(0, 0, 1, 1).to_image();
		}
		else {
			img = img.view(0, 0, self.image.width(), self.image.height()).to_image();
			(avgc, maxc) = imageops::get_avgc_maxc(&img);
		};

		macros::log!(trace, "PaaEncoder::encode: AVGC={}, MAXC={}", avgc, maxc);

		let paatype = self.settings.format;

		let avgc_tagg = Tagg::Avgc { rgba: avgc };
		let maxc_tagg = Tagg::Maxc { rgba: maxc };
		let taggs = vec![avgc_tagg, maxc_tagg];

		let mut mipmaps = imageops
			::construct_mipmap_series(img, 1, image::imageops::FilterType::Triangle)
			.iter()
			.map(|i| PaaMipmap::encode(paatype, i))
			.collect::<Vec<PaaResult<PaaMipmap>>>();
		mipmaps.truncate(<u8 as Into<usize>>::into(PaaImage::MAX_MIPMAPS));

		let image = PaaImage { paatype, taggs, palette: None, mipmaps };

		Ok(image)
	}
}


/// Steps applied to an RGBA image when converting to PAA
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct TextureEncodingSettings {
	/// [`PaaImage::paatype`] of the output PAA.
	pub format: PaaType,
	/// `[TODO]`
	pub dynrange: Option<bool>,
	/// Crop the texture to 1x1 if solid color.
	pub autoreduce: bool,
	/// `[TODO]`
	pub mipmap_filter: Option<TextureMipmapFilter>,
	/// Subpixel mapping applied to the input image.
	pub swizzle: ArgbSwizzle,
	/// `[TODO]`
	pub error_metrics: Option<TextureErrorMetrics>,
}


impl std::fmt::Display for TextureEncodingSettings {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		let mut segments: Vec<String> = vec![];
		segments.push(format!("{:?}", self.format));

		if let Some(r) = self.dynrange {
			segments.push(format!("dynRange={}", r));
		};

		if self.autoreduce {
			segments.push("autoreduce".into());
		};

		if let Some(f) = self.mipmap_filter {
			segments.push(format!("{:?}", f));
		};

		if !self.swizzle.is_noop() {
			segments.push(format!("swizzle=<{}>", self.swizzle));
		};

		if let Some(m) = self.error_metrics {
			segments.push(format!("errorMetrics={:?}", m));
		};

		write!(f, "<{}>", segments.join(", "))
	}
}


/// `[TODO]`
#[allow(missing_docs)]
#[derive(Debug, PartialEq, Eq, Clone, Copy, FromStr)]
#[enumeration(case_insensitive)]
pub enum TextureMipmapFilter {
	AlphaNoise,
	FadeOut,
	AddAlphaNoise,
	NormalizeNormalMap,
	NormalizeNormalMapAlpha,
	NormalizeNormalMapNoise,
	NormalizeNormalMapFade,
}


/// `[TODO]`
#[allow(missing_docs)]
#[derive(Debug, PartialEq, Eq, Clone, Copy, FromStr)]
#[enumeration(case_insensitive)]
pub enum TextureErrorMetrics {
	Distance,
}


/// The file `TexConvert.cfg` from Arma's TexView2, represented as a
/// [suffix string][`String`] &#x21A6; [Settings][`TextureEncodingSettings`] map
///
/// The `TexConvert.cfg` file contains encoding directions for different texture
/// types; different texture types are distinguished by their (case-insensitive)
/// suffix.  The suffix is the last element of the file stem split by `'_'`;
/// e.g., the file `"shoreWetNormal_nohq.paa"` has the texture type suffix
/// "NOHQ"; this texture type is represented by the following
/// `TexConvert.cfg` entry:
///
/// ```text
/// class normalmap_hq {
///   name = "*_nohq.*";
///   format = "DXT5";
///   //negate is used on B channel so that it can used in the same shader as DXT1
///   channelSwizzleA = "1-R";
///   channelSwizzleR = "1-A";
///   channelSwizzleG = "G";
///   channelSwizzleB = "B";
///   dynRange = 0;
///   errorMetrics=Distance;
///   //alpha channel (before swizzle) can be used to contain opacity
///   mipmapFilter = NormalizeNormalMapAlpha;
/// };
/// ```
///
/// The config entry `name` contains the texture suffix in the form of a glob
/// pattern; all entries are of this specific form and can be simplified as the
/// suffix.
#[derive(Debug)]
pub struct TextureHints {
	hints: HashMap<String, TextureEncodingSettings>,
}


impl Deref for TextureHints {
	type Target = HashMap<String, TextureEncodingSettings>;

	fn deref(&self) -> &Self::Target {
		&self.hints
	}
}


impl TextureHints {
	/// Constructs an instance of [`Self`] from the [suffix string][`String`]
	/// &#x21A6; [Settings][`TextureEncodingSettings`] map.
	///
	/// # Example
	/// ```
	/// # use std::path::Path; use std::collections::HashMap;
	/// # use a3_paa::{TextureHints, PaaType, PaaType::*, TextureEncodingSettings};
	/// let mut hints = HashMap::from([("SMDI".to_owned(), TextureEncodingSettings { format: Dxt1, ..Default::default() })]);
	/// let tc = TextureHints::with_hints(hints);
	/// ```
	pub fn with_hints(hints: HashMap<String, TextureEncodingSettings>) -> Self {
		Self { hints }
	}


	/// Construct an instance of [`Self`] from the contents of a `TexConvert.cfg` file.
	///
	/// # Errors
	/// - [`TexconvertParseError`]: Could not parse a list of config items.
	///
	/// # Example
	/// ```no_run
	/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
	/// # use a3_paa::TextureHints;
	/// let tc_contents = std::fs::read_to_string("TexConvert.cfg")?;
	/// let tc = TextureHints::try_parse_from_str(&tc_contents)?;
	/// # Ok(()) }
	/// ```
	pub fn try_parse_from_str(input: &str) -> PaaResult<Self> {
		let hints = cfgfile::try_parse_texconvert(input)?;
		let result = TextureHints { hints };
		Ok(result)
	}


	/// Get the PAA texture type suffix from a PAA path.
	///
	/// # Example
	/// ```
	/// # use a3_paa::TextureHints; use std::path::Path;
	/// assert_eq!(TextureHints::texture_filename_to_suffix(&Path::new("raindrop3_smdi.paa")), Some("SMDI".into()));
	/// ```
	pub fn texture_filename_to_suffix<T: AsRef<std::path::Path>>(path: &T) -> Option<String> {
		let (_, rsplit) = path.as_ref()
			.file_stem()?
			.to_str()?
			.rsplit_once('_')?;
		Some(rsplit.to_uppercase())
	}
}
