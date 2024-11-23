use crate::oc_color::formatters::Formatter;
use lodepng::Bitmap;
use renderers::{CachedRenderer, CodeRenderer, Renderer};

pub use term_pixel::TermPixel;

mod batchers;
mod term_pixel;
mod renderers;
mod term_char;
pub mod szt_file;

type Frame = Bitmap<TermPixel>;

pub fn code_gen(frame: &Frame, prev_frame: Option<&Frame>, formatter: &impl Formatter) -> String {
	let mut renderer = CachedRenderer::new(CodeRenderer::new(
		"gpu".to_string(),
		format!(include_str!("bootstrap.lua"), frame.width, frame.height),
		formatter
	));
	batchers::batcher2::draw(&mut renderer, &frame, prev_frame);
	renderer.into_inner().build()
}
