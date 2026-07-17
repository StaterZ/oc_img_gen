use palette::{Lab, Srgb, rgb::channels::Argb, FromColor};
use lazy_static::lazy_static;

lazy_static! {
	pub static ref BLACK: Srgb<u8> = Srgb::<u8>::from_u32::<Argb>(0x000000);
	pub static ref WHITE: Srgb<u8> = Srgb::<u8>::from_u32::<Argb>(0xFFFFFF);

	pub static ref ZERO_LAB: Lab = Lab::new(0.0, 0.0, 0.0);
	pub static ref BLACK_LAB: Lab = Lab::from_color(BLACK.into_format::<f32>());
	pub static ref WHITE_LAB: Lab = Lab::from_color(WHITE.into_format::<f32>());
}

pub const IMPROVED_DIFFERENCE_MAX: f32 = 40.686831874610654;
