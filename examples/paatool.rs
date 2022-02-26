#![allow(unused_variables)]


use a3_paa::*;


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
			.arg(clap::arg!(serialize_back: -S "Serialize PAA back in memory for debugging").takes_value(false))
			.arg(clap::arg!(input: <INPUT> "PAA file to parse")))
}


fn main() -> Result<(), Box<dyn std::error::Error>> {
	let matches = construct_app().get_matches();
	let loglevel = matches.value_of("loglevel").unwrap_or("Info").parse::<log::LevelFilter>().expect("Could not parse -L<loglevel>");

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


fn command_info(matches: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
	let mut result: PaaResult<_> = Ok(());
	let input = matches.value_of("input").expect("INPUT required");

	let mut file = std::fs::File::open(input)?;
	let filesize = file.metadata().expect("Could not read file metadata").len();
	let image = PaaImage::read_from(&mut file)?;

	println!("{}: File size: {} (0x{:X})", input, filesize, filesize);
	println!("{}: PaaType: {:?}", input, image.paatype);

	for (pos, tagg) in image.taggs.iter().enumerate() {
		println!("{}: Tagg #{}: {:?}", input, pos+1, tagg);
	};

	let mipmaps = image.mipmaps.clone().into_fallible();

	for (pos, m) in mipmaps.iter().enumerate() {
		let pos = pos + 1;

		if let Ok(m) = m {
			println!("{}: Mipmap #{}, {}x{} [{:?}], size={}",
				input,
				pos,
				m.width,
				m.height,
				m.compression,
				m.data.len());
		}
		else {
			if !matches!(m, Err(PaaError::EmptyMipmap)) {
				result = m.clone().map(|_| ());
			};

			println!("{}: Mipmap #{} ERROR {:?}", input, pos, m);
		};
	};

	if matches.is_present("serialize_back") {
		log::trace!("Attempting to serialize PaaImage back");

		let image = image.into_infallible()?;
		let data = image.as_bytes()?;
	};

	result.map_err(|e| e.into())
}


fn command_paa2png(matches: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
	let paa_path = matches.value_of("paa").expect("PAA required");
	let png_path = matches.value_of("png").expect("PNG required");
	let mip_idx = matches.value_of("mipmap").unwrap_or("1").parse::<usize>().expect("Could not parse -m <mipmap>");

	let mut paa_file = std::fs::File::open(paa_path)?;
	let image = PaaImage::read_from(&mut paa_file)?.into_infallible()?;
	let mipmap_count = image.mipmaps.len();

	if !(1..=mipmap_count).contains(&mip_idx) {
		log::error!(
			"Specified mipmap index is out of bounds: requested {}, possible values are in the interval [1, {}]",
			mip_idx,
			mipmap_count);
		return Err(Box::new(PaaError::MipmapIndexOutOfRange));
	};

	let decoder = PaaDecoder::from_paa(image);

	let decoded_image = decoder.decode_nth(mip_idx-1)?;
	decoded_image.save_with_format(png_path, image::ImageFormat::Png)?;

	Ok(())
}
