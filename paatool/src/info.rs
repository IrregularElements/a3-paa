use a3_paa::*;
use anyhow::{Context, Result as AnyhowResult};


pub fn command_info(matches: &clap::ArgMatches) -> AnyhowResult<()> {
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


fn paa_path_info(path: &str, brief: bool, serialize_back: bool) -> AnyhowResult<()> {
	let brief_prefix = if brief {
		"".to_string()
	}
	else {
		format!("{}: ", path)
	};

	let mut file = std::fs::File::open(path).with_context(|| format!("Could not open file: {path}"))?;
	let filesize = file.metadata().with_context(|| format!("Could not read metadata to determine size: {path}"))?.len();
	let image = PaaImage::read_from(&mut file).with_context(|| format!("Could not read PaaImage: {path}"))?;

	println!("{brief_prefix}File size: {filesize} (0x{filesize:X})");
	println!("{brief_prefix}PaaType: {:?}", image.paatype);

	for (pos, tagg) in image.taggs.iter().enumerate() {
		println!("{brief_prefix}Tagg #{}: {tagg}", pos+1);
	};

	let mipmaps = image.mipmaps.clone();

	for (pos, m) in mipmaps.iter().enumerate() {
		let pos = pos + 1;

		if let Ok(m) = m {
			println!("{brief_prefix}Mipmap #{pos}, {}x{} [{:?}], size={}",
				m.width,
				m.height,
				m.compression,
				m.data.len());
		}
		else {
			println!("{brief_prefix}Mipmap #{pos} ERROR {m:?}");
		};
	};

	if serialize_back {
		tracing::trace!("Attempting to serialize PaaImage back");

		let data = image.to_bytes().context("Could not serialize image to bytes")?;
	};

	Ok(())
}
