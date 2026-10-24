use std::sync::LazyLock;
use palette::{Lab, Srgb, rgb::channels::Argb, FromColor};

pub static BLACK: LazyLock<Srgb<u8>> = LazyLock::new(|| Srgb::<u8>::from_u32::<Argb>(0x000000));
pub static WHITE: LazyLock<Srgb<u8>> = LazyLock::new(|| Srgb::<u8>::from_u32::<Argb>(0xFFFFFF));

pub static ZERO_LAB: LazyLock<Lab> = LazyLock::new(|| Lab::new(0.0, 0.0, 0.0));
pub static BLACK_LAB: LazyLock<Lab> = LazyLock::new(|| Lab::from_color(BLACK.into_format::<f32>()));
pub static WHITE_LAB: LazyLock<Lab> = LazyLock::new(|| Lab::from_color(WHITE.into_format::<f32>()));

pub const IMPROVED_DIFFERENCE_MAX: f32 = 40.686831874610654;
