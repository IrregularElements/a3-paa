#![allow(unused_variables)]


use a3_paa::*;
use anyhow::{Context, Result};


fn construct_app() -> clap::Command<'static> {
	clap::Command::new("paatool")
		.version(clap::crate_version!())
		.setting(clap::AppSettings::DeriveDisplayOrder)
		.arg(clap::arg!(loglevel: -L "Global log verbosity level")
			.ignore_case(true)
			.possible_values(["Error", "Warn", "Info", "Debug", "Trace"])
			.default_value("Info"))
		.subcommand(clap::Command::new("paa2png")
			.about("Convert a PAA file to PNG")
			.arg(clap::arg!(mipmap: -m "1-based mipmap index").default_value("1"))
			.arg(clap::arg!(paa: <PAA> "PAA input file"))
			.arg(clap::arg!(png: <PNG> "PNG output path")))
		.subcommand(clap::Command::new("info")
			.about("Parse a PAA file and log details")
			.arg(clap::arg!(brief: -b --brief "Do not prepend file name to output").takes_value(false))
			.arg(clap::arg!(serialize_back: -S "Serialize PAA back in memory for debugging").takes_value(false))
			.arg(clap::arg!(input: <INPUT> "PAA file to parse")))
}


fn paatool() -> Result<()> {
	let matches = construct_app().get_matches();
	let loglevel_str = matches.value_of("loglevel")
		.unwrap_or("Info");
	let loglevel = loglevel_str
		.parse::<log::LevelFilter>()
		.with_context(|| format!("Failed to parse loglevel from -L{}", loglevel_str))?;

	fern::Dispatch::new()
		.format(|out, message, record| {
			out.finish(format_args!(
				"[{}] [{}] {}",
				record.target(),
				record.level(),
				message
			))
		})
		.level(loglevel)
		.chain(std::io::stderr())
		.apply()
		.unwrap();

	log::trace!("Global loglevel set to {:?}", loglevel);

	match matches.subcommand() {
		Some(("paa2png", matches)) => {
			command_paa2png(matches)
		},

		Some(("info", matches)) => {
			command_info(matches)
		},

		Some((&_, _)) => unreachable!(),

		None => {
			let _ = construct_app().print_help();
			Ok(())
		},
	}
}


fn main() -> Result<()> {
	match paatool() {
		Ok(()) => Ok(()),
		Err(e) => { log::error!("{:?}", e); Ok(()) },
	}
}


fn command_info(matches: &clap::ArgMatches) -> Result<()> {
	let input = matches.value_of("input").expect("INPUT required");
	let brief = matches.is_present("brief");

	let brief_prefix = if brief {
		"".to_string()
	}
	else {
		format!("{}: ", input)
	};

	let mut file = std::fs::File::open(input).with_context(|| format!("Could not open file: {}", input))?;
	let filesize = file.metadata().with_context(|| format!("Could not read metadata to determine size: {}", input))?.len();
	let image = PaaImage::read_from(&mut file).with_context(|| format!("Could not read PaaImage: {}", input))?;

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

	if matches.is_present("serialize_back") {
		log::trace!("Attempting to serialize PaaImage back");

		let data = image.as_bytes().context("Could not serialize image to bytes")?;
	};

	Ok(())
}


fn command_paa2png(matches: &clap::ArgMatches) -> Result<()> {
	let paa_path = matches.value_of("paa").expect("PAA required");
	let png_path = matches.value_of("png").expect("PNG required");
	let mip_idx_str = matches.value_of("mipmap").unwrap_or("1");
	let mip_idx = mip_idx_str.parse::<usize>().with_context(|| format!("Failed to parse mipmap index from -m{}", mip_idx_str))?;

	let mut paa_file = std::fs::File::open(paa_path).with_context(|| format!("Could not open file: {}", paa_path))?;
	let image = PaaImage::read_from(&mut paa_file).with_context(|| format!("Could not read PaaImage: {}", paa_path))?;
	let mip_count = image.mipmaps.len();

	let decoder = PaaDecoder::from_paa(image);

	let decoded_image = decoder.decode_nth(mip_idx-1)
		.with_context(|| format!("Failed to decode mipmap #{} (should be in [1..{}])", mip_idx, mip_count))?;
	decoded_image.save_with_format(png_path, image::ImageFormat::Png)
		.with_context(|| format!("save_with_format to path failed: {}", png_path))?;

	Ok(())
}
