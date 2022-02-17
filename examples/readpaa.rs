use std::error::Error;
use a3_paa::PaaImage;


fn main() -> Result<(), Box<dyn Error>> {
	fern::Dispatch::new()
		.format(|out, message, record| {
			out.finish(format_args!(
				"[{}] [{}] {}",
				record.target(),
				record.level(),
				message
			))
		})
		.level(log::LevelFilter::Trace)
		.chain(std::io::stderr())
		.apply()
		.unwrap();

	let mut f = std::fs::File::open(std::env::args().nth(1).unwrap())?;
	let img = PaaImage::read_from(&mut f)?;

	let mut success = true;

	log::info!("File size: {} (0x{:X}); PaaType: {:?}",
		f.metadata().unwrap().len(),
		f.metadata().unwrap().len(),
		img.paatype);

	for (pos, t) in img.taggs.iter().enumerate() {
		log::info!("Tagg #{}: {:?}", pos + 1, t);
	}

	let mipmaps = if let a3_paa::PaaMipmapContainer::Fallible(m) = &img.mipmaps {
		m
	}
	else {
		panic!();
	};

	for (pos, m) in mipmaps.iter().enumerate() {
		let pos = pos + 1;
		if let Ok(m) = m {
			log::info!("Mipmap #{}, width={}, height={}, compression={:?}, data size={}",
				pos,
				m.width,
				m.height,
				m.compression,
				m.data.len());
		}
		else {
			if !matches!(m, Err(a3_paa::PaaError::EmptyMipmap)) {
				success = false;
			};

			log::info!("Mipmap #{} ERROR {:?}", pos, m);
		}
	}

	let img = img.into_infallible()?;

	let data = img.as_bytes().unwrap();

	std::fs::write(std::env::args().nth(2).unwrap_or("/dev/null".to_string()), data).unwrap();

	if !success {
		std::process::exit(1);
	}

	Ok(())
}
