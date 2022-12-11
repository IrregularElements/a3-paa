use surety::Ensure;

use crate::Bgra8888Pixel;
type ImageBuffer = image::ImageBuffer<image::Rgba<u8>, Vec<u8>>;


pub(crate) fn is_solid_color(image: &ImageBuffer) -> bool {
	use image::Pixel;
	let mut pixels = image.pixels();
	let first = if let Some(p) = pixels.next() { p } else { return true; };
	pixels.all(|p| p.channels() == first.channels())
}


pub(crate) fn get_avgc_maxc(image: &ImageBuffer) -> (Bgra8888Pixel, Bgra8888Pixel) {
	if image.dimensions() == (0, 0) {
		return (Default::default(), Default::default());
	};

	let mut pix_count = 0u64.checked();
	let mut avgc: [u64; 4] = [0; 4];
	let mut maxc: [u8; 4] = [0; 4];

	for pixel in image.pixels() {
		for (i, c) in pixel.0.iter().enumerate() {
			avgc[i] += *c as u64;
			maxc[i] = std::cmp::max(maxc[i], *c);
		};

		pix_count += 1;
	};

	let pix_count = pix_count.expect("Pixel count overflows a u64");

	#[allow(clippy::cast_possible_truncation)]
	let avgc = avgc.map(|c: u64| (c / pix_count) as u8);

	(image::Rgba::<u8>(avgc).into(), image::Rgba::<u8>(maxc).into())
}


pub(crate) fn hint_mipmap_count((w, h): (u32, u32), min_dimension: u32) -> usize {
	let smaller = std::cmp::min(w, h) as f64;
	let hint = (smaller.log2() - (min_dimension as f64).log2()).ceil() as usize;
	std::cmp::max(hint, 1usize)
}


#[test]
fn test_hint_mipmap_count() {
	assert_eq!(hint_mipmap_count((800, 1000), 6), 8);
	assert_eq!(hint_mipmap_count((1080, 2160), 30), 6);
}


pub(crate) fn construct_mipmap_series(image: ImageBuffer, min_dimension: u32, filter: image::imageops::FilterType) -> Vec<ImageBuffer> {
	let mut result = Vec::with_capacity(hint_mipmap_count(image.dimensions(), min_dimension));
	let mut current = image;

	loop {
		let (width, height) = current.dimensions();

		if width < min_dimension || height < min_dimension {
			break;
		};

		result.push(current.clone());

		current = image::imageops::resize(&current, width / 2, height / 2, filter);
	};

	result
}
