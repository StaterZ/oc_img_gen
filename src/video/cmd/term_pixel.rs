use num::One;

use crate::math::*;

use super::{
	super::{braille::Braille, oc_color::{PackedColor, formatters::Formatter}},
	TermChar,
};

#[derive(Debug, Eq, Clone, Copy)]
pub struct TermPixel {
	pub sym: TermChar,
	pub bg: PackedColor,
	pub fg: PackedColor,
}

impl TermPixel {
	pub fn flip(&self) -> Option<Self> {
		self.sym.flip().map(|sym| Self {
			sym,
			bg: self.fg,
			fg: self.bg,
		})
	}

	pub fn to_canonical(&self) -> Self {
		match u8::from(self.bg).cmp(&self.fg.into()) {
			std::cmp::Ordering::Less => *self,
			std::cmp::Ordering::Equal => Self {
				sym: ' '.into(),
				bg: self.bg,
				fg: self.fg, //arbitrary
			},
			std::cmp::Ordering::Greater => if let Some(flipped_char) = self.flip() {
				flipped_char
			} else {
				*self
			},
		}
	}
	
	fn basic_eq(&self, other: &Self) -> bool {
		self.sym == other.sym && self.bg == other.bg && self.fg == other.fg
	}

	pub fn compute_loss(&self, other: &Self, formatter: &impl Formatter) -> Frac<u64> {
		if ('⠀'..='⣿').contains(&char::from(self.sym)) {
			let bg_bg_delta = formatter.inflate(self.bg).perceptual_delta(formatter.inflate(other.bg));
			let bg_fg_delta = formatter.inflate(self.bg).perceptual_delta(formatter.inflate(other.fg));
			let fg_bg_delta = formatter.inflate(self.fg).perceptual_delta(formatter.inflate(other.bg));
			let fg_fg_delta = formatter.inflate(self.fg).perceptual_delta(formatter.inflate(other.fg));

			const BASE: u32 = '⠀' as u32;
			let self_sym_val = (char::from(self.sym) as u32 - BASE) as u8;
			let other_sym_val = (char::from(other.sym) as u32 - BASE) as u8;
			let score =
				(!self_sym_val & !other_sym_val).count_ones() * bg_bg_delta +
				(!self_sym_val & other_sym_val).count_ones() * bg_fg_delta +
				(self_sym_val & !other_sym_val).count_ones() * fg_bg_delta +
				(self_sym_val & other_sym_val).count_ones() * fg_fg_delta;
			const PERCEPTUAL_DELTA_MAX: u32 = 650250000;
			Frac::new(score, PERCEPTUAL_DELTA_MAX).cast::<u64>()
		} else {
			Frac::one() //TODO: should we handle loss for text?
		}
	}
}

impl PartialEq for TermPixel {
	fn eq(&self, other: &Self) -> bool {
		self.to_canonical().basic_eq(&other.to_canonical())
	}
}

impl From<Braille<PackedColor>> for TermPixel {
	fn from(value: Braille<PackedColor>) -> Self {
		Self {
			sym: value.char().into(),
			bg: value.bg,
			fg: value.fg,
		}
	}
}
