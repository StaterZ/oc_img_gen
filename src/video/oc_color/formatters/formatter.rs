use palette::Lab;

use super::super::{palette::PaletteOr, PackedColor};

pub trait Formatter {
	fn inflate(&self, color: PackedColor) -> Lab;
	fn deflate(&self, color: PaletteOr<Lab>) -> PackedColor;
}
