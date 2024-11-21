use phf::phf_map;

macro_rules! phf_bidirectional_map {
	($($k:expr => $v:expr),* $(,)?) => {
		phf_map! {
			$(
				$k => $v,
				$v => $k,
			)*
		}
	};
}

static FLIPPABLES: phf::Map<char, char> = phf_bidirectional_map! {
	' ' => '█',
	'◤' => '◢',
	'◣' => '◥',
	'▄' => '▀',
	'▌' => '▐',
	'▉' => '▕',
	'▇' => '▔',
	'▘' => '▟',
	'▝' => '▙',
	'▗' => '▛',
	'▖' => '▜',
	'▞' => '▚',
};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct TermChar(char);

impl TermChar {
	pub fn into_inner(self) -> char {
		self.0
	}

	pub fn flip(&self) -> Option<TermChar> {
		if ('⠀'..='⣿').contains(&self.0) {
			const BASE: u32 = '⠀' as u32;
			let flipped = (!((self.0 as u32) - BASE) & 0xFF) + BASE;
			char::from_u32(flipped)
		} else {
			FLIPPABLES.get(&self.0).copied()
		}.map(|c| c.into())
	}
	
	pub fn is_bg_only(&self) -> bool {
		matches!(self.0, ' ' | '⠀')
	}
	
	pub fn is_fg_only(&self) -> bool {
		matches!(self.0, '▇' | '⣿')
	}
}

impl From<char> for TermChar {
	fn from(value: char) -> Self {
		Self(value)
	}
}

impl Into<char> for TermChar {
	fn into(self) -> char {
		self.into_inner()
	}
}
