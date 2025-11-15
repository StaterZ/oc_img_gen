use super::{
	super::{braille::Braille, oc_color::PackedColor},
	TermChar,
};

#[derive(Debug, PartialEq, Eq, Clone)]
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
			std::cmp::Ordering::Less => self.clone(),
			std::cmp::Ordering::Equal => Self {
				sym: ' '.into(),
				bg: self.bg,
				fg: self.fg, //arbitrary
			},
			std::cmp::Ordering::Greater => if let Some(flipped_char) = self.flip() {
				flipped_char
			} else {
				self.clone()
			},
		}
	}
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
