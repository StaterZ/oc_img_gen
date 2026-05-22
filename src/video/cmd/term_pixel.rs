use super::{
	super::{braille::Braille, oc_color::PackedColor},
	TermChar,
};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
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
		match <PackedColor as Into<u8>>::into(self.bg).cmp(&self.fg.into()) {
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
	
	// pub fn compute_loss(&self, other: &Self, renderer: &HybridFormatter) -> u32 {
	// 	if ('⠀'..='⣿').contains(&self.sym.0) {
	// 		let bgbg_delta = renderer.inflate(self.bg).perceptual_delta(renderer.inflate(other.bg));
	// 		let fgfg_delta = renderer.inflate(self.fg).perceptual_delta(renderer.inflate(other.fg));
	// 		let bgfg_delta = renderer.inflate(self.bg).perceptual_delta(renderer.inflate(other.fg));
	// 		let fgbg_delta = renderer.inflate(self.fg).perceptual_delta(renderer.inflate(other.bg));

	// 		const BASE: u32 = '⠀' as u32;
	// 		let self_sym_val = ((self.sym.0 as u32) - BASE) as u8;
	// 		let other_sym_val = ((other.sym.0 as u32) - BASE) as u8;
	// 		let score =
	// 			(self_sym_val & other_sym_val) as u32 * fgfg_delta +
	// 			(!self_sym_val & !other_sym_val) as u32 * bgbg_delta +
	// 			(self_sym_val & !other_sym_val) as u32 * fgbg_delta +
	// 			(!self_sym_val & other_sym_val) as u32 * bgfg_delta;
	// 		score
	// 	} else {
	// 		u32::MAX //TODO: should we handle loss for text?
	// 	}
	// }
}


impl From<&Braille<PackedColor>> for TermPixel {
	fn from(value: &Braille<PackedColor>) -> Self {
		Self {
			sym: value.char().into(),
			bg: value.bg,
			fg: value.fg,
		}
	}
}
