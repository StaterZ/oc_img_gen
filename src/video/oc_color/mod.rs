//source: https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/util/PackedColor.scala

// #[allow(unused_imports)]
// pub use rgb::{RGB, RGB8};
#[allow(unused_imports)]
pub use palette::{PaletteOr, PaletteColor};
#[allow(unused_imports)]
pub use packed_color::PackedColor;

pub mod formatters;

mod palette;
mod packed_color;
