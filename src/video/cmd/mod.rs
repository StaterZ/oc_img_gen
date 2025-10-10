use super::{braille::Braille, image::Image, oc_color::{formatters::Formatter, PackedColor}};
use renderers::{CachedRenderer, CodeRenderer, Renderer};
pub use term_pixel::TermPixel;
pub use term_char::TermChar;
pub use machine::Machine;

mod batchers;
mod term_pixel;
mod renderers;
mod term_char;
mod machine;
pub mod packet;

type TermFrame = Image<TermPixel>;
type BrailleFrame = Image<Braille<PackedColor>>;

pub fn code_gen(frame: &TermFrame, prev_frame: Option<&TermFrame>, formatter: &impl Formatter) -> String {
	let mut renderer = CachedRenderer::new(CodeRenderer::new(
		"gpu".to_string(),
		format!(include_str!("bootstrap.lua"), frame.size().x, frame.size().y),
		formatter
	));
	batchers::batcher_v2::draw(&mut renderer, &frame, prev_frame);
	renderer.into_inner().build()
}
