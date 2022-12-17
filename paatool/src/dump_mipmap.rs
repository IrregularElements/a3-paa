use std::io::{Seek as _, SeekFrom, prelude::*};

use a3_paa::*;
use anyhow::{Context, Result as AnyhowResult};
use byteorder::{ReadBytesExt as _, LittleEndian};


pub fn command_dump_mipmap(matches: &clap::ArgMatches) -> AnyhowResult<()> {
	let paa_path = matches.value_of("paa").expect("PAA required");
	let bin_path = matches.value_of("bin").expect("BIN required");
	let compressed = matches.is_present("compressed");
	let mip_idx_str = matches.value_of("mipmap")
		.unwrap_or("1");
	let mip_idx = mip_idx_str.parse::<usize>()
		.context(format!("Could not parse mipmap index from \"{mip_idx_str}\""))
		.and_then(|i| if i > 0 { Ok(i) } else { Err(anyhow::anyhow!("Mipmap index cannot be 0")) })?;

	tracing::trace!("Mipmap #{mip_idx} requested");

	let mut paa_file = std::fs::File::open(paa_path)
		.context(format!("{paa_path}: Could not open file"))?;
	let image = PaaImage::read_from(&mut paa_file)
		.context(format!("{paa_path}: Could not read PaaImage"))?;

	match compressed {
		false => {
			let mipmap = image.mipmaps.get(mip_idx-1)
				.context("Mipmap index out of range")?
				.to_owned()
				.context("Mipmap read error")?;

			std::fs::write(bin_path, &mipmap.data)
				.context(format!("{bin_path}: Could not write mipmap data"))?;
		},

		true => {
			tracing::trace!("Using OFFSTAGG to read raw mipmap data");

			let offs = image.taggs.iter()
				.find(|t| matches!(t, a3_paa::Tagg::Offs { offsets: _ }))
				.context("OFFSTAGG not found")?;
			let offsets = match offs {
				a3_paa::Tagg::Offs { offsets } => offsets,
				_ => unreachable!(),
			};

			tracing::trace!("OFFSTAGG found: {offs:?}");

			let offset = offsets.get(mip_idx-1)
				.context("Mipmap index out of range of OFFSTAGG")?;

			tracing::trace!("Mipmap offset is 0x{offset:02X}");

			paa_file.seek(SeekFrom::Start((*offset).into()))
				.context(format!("{paa_path}: Failed to seek to {offset}"))?;

			let w = paa_file.read_u16::<LittleEndian>()
				.context("Could not read mipmap width")?;
			let h = paa_file.read_u16::<LittleEndian>()
				.context("Could not read mipmap height")?;
			let l = paa_file.read_uint::<LittleEndian>(3)
				.context("Could not read mipmap size")? as usize;
			tracing::trace!("Mipmap #{mip_idx}: {w}x{h}, data length={l}");
			let mut data: Vec<u8> = vec![0; l];
			paa_file.read_exact(&mut data)
				.context("Could not read mipmap data")?;
			std::fs::write(bin_path, &data)
				.context(format!("{bin_path}: Could not write mipmap data"))?;
		},
	};


	Ok(())
}
