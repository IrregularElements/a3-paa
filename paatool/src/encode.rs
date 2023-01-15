use std::path::PathBuf;

use a3_paa::*;
use anyhow::{Context, anyhow, Result as AnyhowResult};
use tap::prelude::*;


const ARMA3_TOOLS_STEAM_APPID: u32 = 233880;


pub fn command_encode(matches: &clap::ArgMatches) -> AnyhowResult<()> {
	let img_path = matches.value_of("img").expect("IMG required");
	let paa_path = matches.value_of("paa").expect("PAA required");

	let hints_str: String = if let Some(path) = matches.value_of("hints") {
		std::fs::read_to_string(&path)
			.context(format!("{path:?}: Failed to read TexConvert.cfg"))?
	}
	else {
		suggest_hints_paths()
			.find_map(|p| std::fs::read_to_string(&p).ok())
			.tap_some(|p| tracing::trace!("Located TexConvert.cfg at path: {p:?}"))
			.context("No TexConvert.cfg file provided, and could not locate any")?
	};

	let hints = TextureHints
		::try_parse_from_str(&hints_str)
		.tap_ok(|h| tracing::trace!("Parsed TexConvert.cfg; got {} hints", h.len()))
		.context("Failed to parse TexConvert.cfg")?;

	let paa_path_suffix = TextureHints
		::texture_filename_to_suffix(&paa_path)
		.context(format!("{paa_path:?}: No suffix in texture path"));

	let suffix = matches.value_of("suffix")
		.map(String::from)
		.ok_or_else(|| anyhow!("SUFFIX not specified"))
		.or(paa_path_suffix)
		.context("Texture suffix was not specified and not found in texture path")?;

	let image = image::open(img_path)
		.context(format!("{img_path:?}: Failed to open input IMG"))?
		.into_rgba8();

	let settings = hints
		.get(&suffix)
		.context(format!("{suffix:?}: Texture type not found in config"))?;
	tracing::info!("Texture settings for {paa_path:?}: {settings}");

	let warn_unimplemented = |path, prop| tracing::error!("{path}: Texture has `{prop}` \
		set, which is currently not implemented; ignoring it and continuing");

	if settings.dynrange.is_some() {
		warn_unimplemented(paa_path, "dynRange");
	};

	if settings.mipmap_filter.is_some() {
		warn_unimplemented(paa_path, "mipmapFilter");
	};

	if settings.error_metrics.is_some() {
		warn_unimplemented(paa_path, "errorMetrics");
	};

	let encoder = PaaEncoder::with_image_and_settings(image, *settings);

	let paa = encoder.encode()
		.context("Failed to encode image")?;
	let data = paa.to_bytes()
		.context("Failed to serialize PAA to bytes")?;

	std::fs::write(paa_path, data)
		.context(format!("Failed to write PAA data to {paa_path:?}"))?;

	Ok(())
}


fn suggest_hints_paths() -> impl Iterator<Item=PathBuf> {
	fn append_file(p: PathBuf) -> impl Iterator<Item=PathBuf> {
		let with_last = |f: &str| p.clone().tap_mut(|p| p.push(f));
		let dirs: Vec<PathBuf> = vec![with_last("TexConvert.cfg"), with_last("texconvert.cfg")];
		dirs.into_iter()
	}

	let mut parent_dirs: Vec<PathBuf> = vec![];

	if let Ok(cwd) = std::env::current_dir() {
		parent_dirs.push(cwd);
	};

	#[cfg(windows)]
	{
		// [TODO]: Use Arma 3 registry key
		parent_dirs.push(PathBuf::from(r"P:\"));
		parent_dirs.push(PathBuf::from(r"P:\TexView2"));
	};

	steamlocate::SteamDir::locate()
		.and_then(|mut d| d.app(&ARMA3_TOOLS_STEAM_APPID).cloned())
		.map(|a| a.path)
		.tap_some(|p| parent_dirs.push(p.join("TexView2")));

	parent_dirs
		.into_iter()
		.flat_map(append_file)
}
