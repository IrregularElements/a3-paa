#![allow(unused_variables)]

use std::process::ExitCode;

use anyhow::{Context, Result as AnyhowResult};
use tap::prelude::*;

mod encode;
mod decode;
mod info;
mod dds2paa;


fn construct_app() -> clap::Command<'static> {
	clap::Command::new("paatool")
		.version(clap::crate_version!())
		.setting(clap::AppSettings::DeriveDisplayOrder)
		.arg(clap::arg!(loglevel: -L "Global log verbosity level")
			.ignore_case(true)
			.possible_values(["Error", "Warn", "Info", "Debug", "Trace"])
			.default_value("Info"))
		.subcommand(clap::Command::new("encode")
			.about("Encode an image file to PAA")
			.arg(clap::arg!(hints: --hints <HINTS> "TexConvert.cfg file with texture hints")
				.required(false))
			.arg(clap::arg!(suffix: -S --suffix <SUFFIX> "Texture type suffix (e.g. \"CA\"); extracted from PAA if unspecified")
				.required(false))
			.arg(clap::arg!(img: <IMG> "IMG input file"))
			.arg(clap::arg!(paa: <PAA> "PAA output path")))
		.subcommand(clap::Command::new("decode")
			.about("Decode a PAA file to PNG")
			.arg(clap::arg!(mipmap: -m "1-based mipmap index").default_value("1"))
			.arg(clap::arg!(paa: <PAA> "PAA input file"))
			.arg(clap::arg!(png: <PNG> "PNG output path")))
		.subcommand(clap::Command::new("dds2paa")
			.about("Convert a DirectX DDS file to PAA")
			.arg(clap::arg!(layer: -l "1-based array layer index").default_value("1"))
			.arg(clap::arg!(dds: <DDS> "DDS input file"))
			.arg(clap::arg!(paa: <PAA> "PAA output path")))
		.subcommand(clap::Command::new("info")
			.about("Parse a PAA file and log details")
			.arg(clap::arg!(brief: -b --brief "Do not prepend file name to output").takes_value(false))
			.arg(clap::arg!(serialize_back: -S "Serialize PAA back in memory for debugging").takes_value(false))
			.arg(clap::arg!(input: <INPUT> ... "PAA file to parse")))
}


fn paatool() -> AnyhowResult<()> {
	let matches = construct_app().get_matches_from(wild::args());
	let loglevel_str = matches.value_of("loglevel")
		.unwrap_or("Info");
	let loglevel = loglevel_str
		.parse::<tracing::Level>()
		.with_context(|| format!("Failed to parse loglevel from -L{loglevel_str}"))?;

	tracing_subscriber::fmt()
		.with_max_level(loglevel)
		.init();

	tracing::trace!("Global loglevel set to {loglevel:?}");

	match matches.subcommand() {
		Some(("encode", matches)) => {
			encode::command_encode(matches)
		},

		Some(("decode", matches)) => {
			decode::command_decode(matches)
		},

		Some(("info", matches)) => {
			info::command_info(matches)
		},

		Some(("dds2paa", matches)) => {
			dds2paa::command_dds2paa(matches)
		},

		Some((&_, _)) => unreachable!(),

		None => {
			let _ = construct_app().print_help();
			Ok(())
		},
	}
}


fn main() -> ExitCode {
	let report_chain = |e: &anyhow::Error| {
		for (index, cause) in e.chain().enumerate() {
			let suffix = if index == 0 { "" } else { "... " };
			tracing::error!("{suffix}{cause}");
		};
	};

	crate::paatool()
		.tap_err(|e| report_chain(e))
		.map_or(ExitCode::FAILURE, |_| ExitCode::SUCCESS)
}
