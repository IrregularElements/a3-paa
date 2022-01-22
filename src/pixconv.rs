use byteorder::*;


pub(crate) fn argb4444_to_rgba8888(data4: &[u8]) -> Vec<u8> {
	assert_eq!(data4.len() % 2, 0, "Truncated ARGB4444 data in input");

	let mut result = Vec::with_capacity(data4.len()*2);

	for pixel in data4.chunks(2) {
		let pixel = LittleEndian::read_u16(pixel).to_be_bytes();
		let pixel = &pixel;

		let a: u8 = pixel[0] >> 4;
		let r: u8 = pixel[0] & 0x0F;
		let g: u8 = pixel[1] >> 4;
		let b: u8 = pixel[1] & 0x0F;

		let r: u8 = ((r as u16 * 0xFF + 0x1) / 0x0F) as u8;
		let g: u8 = ((g as u16 * 0xFF + 0x1) / 0x0F) as u8;
		let b: u8 = ((b as u16 * 0xFF + 0x1) / 0x0F) as u8;
		let a: u8 = ((a as u16 * 0xFF + 0x1) / 0x0F) as u8;

		result.extend([r, g, b, a]);
	};

	result
}


pub(crate) fn argb1555_to_rgba8888(data5: &[u8]) -> Vec<u8> {
	assert_eq!(data5.len() % 2, 0, "Truncated ARGB1555 data in input");

	let mut result = Vec::with_capacity(data5.len()*2);

	for pixel in data5.chunks(2) {
		let pixel = LittleEndian::read_u16(pixel).to_be_bytes();
		let pixel = &pixel;

		let a: u8 = pixel[0] >> 7;
		let r: u8 = (pixel[0] >> 2) & 0x1F;
		let g: u8 = (pixel[0] << 3 | pixel[1] >> 5) & 0x1F;
		let b: u8 = pixel[1] & 0x1F;

		let r: u8 = ((r as u16 * 0xFF + 0xF) / 0x1F) as u8;
		let g: u8 = ((g as u16 * 0xFF + 0xF) / 0x1F) as u8;
		let b: u8 = ((b as u16 * 0xFF + 0xF) / 0x1F) as u8;
		let a: u8 = a * 0xFF;

		result.extend([r, g, b, a]);
	};

	result
}


pub(crate) fn argb8888_to_rgba8888(data8: &[u8]) -> Vec<u8> {
	assert_eq!(data8.len() % 4, 0, "Truncated ARGB8888 data in input");

	let mut result = Vec::with_capacity(data8.len());

	for pixel in data8.chunks(4) {
		result.extend(pixel.iter().rev());
	};

	result
}
