use crate::{braille::Braille, oc_color::{formatters::Formatter, PackedColor}};
use lodepng::Bitmap;
use renderers::{CachedRenderer, CodeRenderer, Renderer};

pub use term_pixel::TermPixel;

mod batchers;
mod term_pixel;
mod renderers;
mod term_char;
pub mod szt;

type TermFrame = Bitmap<TermPixel>;
type BrailleFrame = Bitmap<Braille<PackedColor>>;

pub fn code_gen(frame: &TermFrame, prev_frame: Option<&TermFrame>, formatter: &impl Formatter) -> String {
	let mut renderer = CachedRenderer::new(CodeRenderer::new(
		"gpu".to_string(),
		format!(include_str!("bootstrap.lua"), frame.width, frame.height),
		formatter
	));
	batchers::batcher2::draw(&mut renderer, &frame, prev_frame);
	renderer.into_inner().build()
}
