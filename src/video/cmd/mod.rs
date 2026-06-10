use crate::math::Frac;

use super::{braille::Braille, image::Image, oc_color::{formatters::Formatter, PackedColor}};
use renderers::{CachedRenderer, CodeRenderer, Renderer};
pub use term_pixel::TermPixel;
pub use term_char::TermChar;

mod batcher;
mod term_pixel;
mod renderers;
mod term_char;
pub mod machine;
pub mod packet;

type TermFrame = Image<TermPixel>;
type BrailleFrame = Image<Braille<PackedColor>>;

pub fn code_gen(frame: &TermFrame, prev_frame: Option<&TermFrame>, acceptable_loss: Frac<u64>, formatter: &impl Formatter) -> String {
	let mut renderer = CachedRenderer::new(CodeRenderer::new(
		"gpu".to_string(),
		format!(include_str!("bootstrap.lua"), frame.size().w, frame.size().h),
		formatter
	));
	batcher::draw(&mut renderer, &frame, prev_frame, 64, acceptable_loss, formatter);
	renderer.into_inner().build()
}
