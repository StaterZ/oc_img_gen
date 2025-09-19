use super::super::{RGB8, palette::PaletteOr, PackedColor};

pub trait Formatter {
	fn inflate(&self, color: PackedColor) -> RGB8;
	fn deflate(&self, color: PaletteOr<RGB8>) -> PackedColor;
}
