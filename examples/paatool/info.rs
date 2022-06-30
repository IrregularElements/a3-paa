use a3_paa::*;
use anyhow::{Context, Result};


pub fn command_info(matches: &clap::ArgMatches) -> Result<()> {
	let brief = matches.is_present("brief");
	let serialize = matches.is_present("serialize_back");

	let mut result = Ok(());

	for path in matches.values_of("input").expect("INPUT required") {
		let result_now = paa_path_info(path, brief, serialize);

		if let Err(ref e) = result_now {
			result = result_now;
		};
	};

	result
}


fn paa_path_info(path: &str, brief: bool, serialize_back: bool) -> Result<()> {
	let brief_prefix = if brief {
		"".to_string()
	}
	else {
		format!("{}: ", path)
	};

	let mut file = std::fs::File::open(path).with_context(|| format!("Could not open file: {}", path))?;
	let filesize = file.metadata().with_context(|| format!("Could not read metadata to determine size: {}", path))?.len();
	let image = PaaImage::read_from(&mut file).with_context(|| format!("Could not read PaaImage: {}", path))?;

	println!("{}File size: {} (0x{:X})", brief_prefix, filesize, filesize);
	println!("{}PaaType: {:?}", brief_prefix, image.paatype);

	for (pos, tagg) in image.taggs.iter().enumerate() {
		println!("{}Tagg #{}: {}", brief_prefix, pos+1, tagg);
	};

	let mipmaps = image.mipmaps.clone();

	for (pos, m) in mipmaps.iter().enumerate() {
		let pos = pos + 1;

		if let Ok(m) = m {
			println!("{}Mipmap #{}, {}x{} [{:?}], size={}",
				brief_prefix,
				pos,
				m.width,
				m.height,
				m.compression,
				m.data.len());
		}
		else {
			println!("{}Mipmap #{} ERROR {:?}", brief_prefix, pos, m);
		};
	};

	if serialize_back {
		tracing::trace!("Attempting to serialize PaaImage back");

		let data = image.to_bytes().context("Could not serialize image to bytes")?;
	};

	Ok(())
}
