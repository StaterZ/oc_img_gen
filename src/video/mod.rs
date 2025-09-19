pub mod oc_color;
pub mod braille;
pub mod cmd;
pub mod image;

#[cfg(feature = "debug-mode")]
fn write_image(path: impl AsRef<Path>, img: &Image<RGB8>) {
	std::fs::write(path, lodepng::encode24(
		&img
			.buffer()
			.iter()
			.map(|p| lodepng::RGB::new(p.r, p.g, p.b))
			.collect_vec(),
		img.size().x,
		img.size().y,
	).unwrap()).unwrap();
}
