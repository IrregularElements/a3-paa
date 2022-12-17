use a3_paa::*;
use anyhow::{Context, Result as AnyhowResult};


pub fn command_decode(matches: &clap::ArgMatches) -> AnyhowResult<()> {
	let paa_path = matches.value_of("paa").expect("PAA required");
	let png_path = matches.value_of("png").expect("PNG required");
	let mip_idx_str = matches.value_of("mipmap").unwrap_or("1");
	let mip_idx = mip_idx_str.parse::<usize>()
		.with_context(|| format!("Could not parse mipmap index from \"{mip_idx_str}\""))
		.and_then(|i| if i > 0 { Ok(i) } else { Err(anyhow::anyhow!("Mipmap index cannot be 0")) })?;

	let mut paa_file = std::fs::File::open(paa_path).with_context(|| format!("Could not open file: {paa_path}"))?;
	let image = PaaImage::read_from(&mut paa_file).with_context(|| format!("Could not read PaaImage: {paa_path}"))?;
	let mip_count = image.mipmaps.len();

	let decoder = PaaDecoder::with_paa(image);

	let decoded_image = decoder.decode_nth(mip_idx-1)
		.with_context(|| format!("Failed to decode mipmap #{mip_idx} (should be in [1..{mip_count}])"))?;
	decoded_image.save_with_format(png_path, image::ImageFormat::Png)
		.with_context(|| format!("save_with_format to path failed: {png_path}"))?;

	Ok(())
}
