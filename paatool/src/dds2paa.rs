use std::fs::File;

use a3_paa::{PaaType, PaaResult, PaaMipmap, PaaImage};
use anyhow::{Context, Result as AnyhowResult};
use ddsfile::{Dds, D3DFormat};
use tap::prelude::*;


pub fn command_dds2paa(matches: &clap::ArgMatches) -> AnyhowResult<()> {
	let dds_path = matches.value_of("dds").expect("DDS required");
	let paa_path = matches.value_of("paa").expect("PAA required");
	let layer = matches
		.value_of("layer")
		.map_or(Ok(1), |l| l.parse::<u32>().context(format!("Could not parse layer index: {l}")))
		.tap_ok(|i| tracing::trace!("Requested layer: {i}"))?;

	let dds_file = File::open(dds_path)
		.context(format!("{dds_path}: Could not open DDS file"))?;
	let dds = Dds::read(dds_file)
		.context(format!("{dds_path}: Could not parse DDS file"))?;

	let d3dfmt = dds.get_d3d_format().map_or("None".into(), |f| format!("{f:?}"));
	let dxgifmt = dds.get_dxgi_format().map_or("None".into(), |f| format!("{f:?}"));
	let (w, h) = (dds.get_width(), dds.get_height());
	let levels = dds.get_num_array_layers();
	let mips = dds.get_num_mipmap_levels();
	tracing::info!("{dds_path}: {d3dfmt}/{dxgifmt}, {w}x{h}, {levels} layers, {mips} mipmaps");

	#[allow(deprecated)]
	let paatype = match dds.get_d3d_format() {
		Some(D3DFormat::DXT1) => PaaType::Dxt1,
		Some(D3DFormat::DXT2) => PaaType::Dxt2,
		Some(D3DFormat::DXT3) => PaaType::Dxt3,
		Some(D3DFormat::DXT4) => PaaType::Dxt4,
		Some(D3DFormat::DXT5) => PaaType::Dxt5,
		f => anyhow::bail!("DDS to PAA conversion not implemented for this D3D format: {f:?}"),
	};

	let data = dds.get_data(layer-1)
		.context(format!("Could not get data for layer {layer}"))?;
	let mut width: u16 = w.try_into().context("Width overflows a u16")?;
	let mut height: u16 = h.try_into().context("Height overflows a u16")?;
	let mut mip_size = paatype.predict_size(width, height);
	let mut cursor: usize = 0;
	let mut mipmaps: Vec<PaaResult<PaaMipmap>> = vec![];

	for i in 0..mips {
		let compression = PaaMipmap::suggest_compression(paatype, width, height);
		let left = cursor;
		let right = cursor + mip_size;
		let data = &data[left..right];
		let mip = PaaMipmap { width, height, compression, paatype, data: data.to_owned() };
		mipmaps.push(Ok(mip));

		cursor += mip_size;
		mip_size /= 4;
		width /= 2;
		height /= 2;
	};

	let paa = PaaImage { paatype, taggs: vec![], palette: None, mipmaps };
	let data = paa.to_bytes().context("Could not serialize PAA")?;
	std::fs::write(paa_path, &data).context("{paa_path}: Could not write PAA data")?;

	Ok(())
}
