//! the tracks to encode, one spec per track. a spec comes from the track flags after --video, --audio or --pcm,
//! or from the tracks config file, which uses the flag names as keys. keys that are left out use the defaults

use std::{fmt::Display, path::Path, str::FromStr};
use anyhow::Context;
use clap::ValueEnum;
use serde::{Deserialize, Deserializer, de::Error};

use crate::{encoder::cli::{BrailleStrategy, Budget, ColorDiffStrategy, VideoFilter}, math::{Frac, Point, Size}};

/// the config file can give a value as text or as a number
fn text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
	#[derive(Deserialize)]
	#[serde(untagged)]
	enum Text {
		Str(String),
		Int(i64),
		Uint(u64),
		Float(f64),
	}

	Ok(Option::<Text>::deserialize(deserializer)?.map(|text| match text {
		Text::Str(text) => text,
		Text::Int(value) => value.to_string(),
		Text::Uint(value) => value.to_string(),
		Text::Float(value) => value.to_string(),
	}))
}

pub fn parse<'de, D: Deserializer<'de>, T: FromStr<Err: Display>>(deserializer: D) -> Result<Option<T>, D::Error> {
	text(deserializer)?
		.map(|text| text.parse().map_err(|e| D::Error::custom(format!("'{text}': {e}"))))
		.transpose()
}

pub fn parse_enum<'de, D: Deserializer<'de>, T: ValueEnum>(deserializer: D) -> Result<Option<T>, D::Error> {
	text(deserializer)?
		.map(|text| T::from_str(&text, true).map_err(|_| D::Error::custom(format!(
			"'{text}' is not one of: {}",
			T::value_variants().iter().filter_map(|value| value.to_possible_value()).map(|value| value.get_name().to_string()).collect::<Vec<_>>().join(", "),
		))))
		.transpose()
}

/// a sound card voices track
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AudioSpec {
	#[serde(default, deserialize_with = "parse")] pub name: Option<String>,
	#[serde(default, deserialize_with = "parse")] pub voices: Option<usize>,
	#[serde(default, deserialize_with = "parse")] pub analysis_rate: Option<u32>,
	#[serde(default, deserialize_with = "parse")] pub fft_window_size: Option<usize>,
	#[serde(default, deserialize_with = "parse")] pub hop_length: Option<usize>,
	#[serde(default, deserialize_with = "parse")] pub guard: Option<isize>,
}

/// a DFPWM track for computronics tape drives
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct PcmSpec {
	#[serde(default, deserialize_with = "parse")] pub name: Option<String>,
	#[serde(default, deserialize_with = "parse")] pub rate: Option<u32>,
	#[serde(default, deserialize_with = "parse")] pub align: Option<u32>,
}

/// the name a track gets when its spec has none. a lone track of its kind is "main"
pub fn default_name(name: Option<String>, index: usize, count: usize) -> String {
	name.unwrap_or_else(|| if count == 1 { "main".to_string() } else { index.to_string() })
}

/// a video track. with `at` it shows that part of the canvas, without it shows the whole canvas.
/// with `matrix` it is a grid of tracks, named "x,y" (or "name:x,y")
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct VideoSpec {
	#[serde(default, deserialize_with = "parse")] pub name: Option<String>,
	#[serde(default, deserialize_with = "parse")] pub size: Option<Size<usize>>,
	#[serde(default, deserialize_with = "parse")] pub at: Option<Offset>,
	#[serde(default, deserialize_with = "parse")] pub matrix: Option<Size<usize>>,
	#[serde(default, deserialize_with = "parse")] pub gap: Option<Size<usize>>,
	#[serde(default, deserialize_with = "parse")] pub screen: Option<Size<usize>>,
	#[serde(default, deserialize_with = "parse")] pub frame_rate: Option<Frac<u16>>,
	#[serde(default, deserialize_with = "parse_enum")] pub filter: Option<VideoFilter>,
	#[serde(default, deserialize_with = "parse_enum")] pub braille: Option<BrailleStrategy>,
	#[serde(default, deserialize_with = "parse_enum")] pub diff: Option<ColorDiffStrategy>,
	#[serde(default, deserialize_with = "parse_enum")] pub budget: Option<Budget>,
	#[serde(default, deserialize_with = "parse")] pub loss: Option<Frac<u32>>,
	#[serde(default, deserialize_with = "parse")] pub loss_step: Option<Frac<u32>>,
}

/// where a track is on the canvas in sub-pixels, as "X+Y"
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offset(pub Point<usize>);

impl FromStr for Offset {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let (x, y) = s.trim().trim_start_matches('+').split_once('+').ok_or_else(|| format!("'{s}' is not X+Y"))?;
		let parse = |value: &str| value.trim().parse::<usize>().map_err(|e| format!("'{value}': {e}"));
		Ok(Self(Point::new(parse(x)?, parse(y)?)))
	}
}

impl Display for Offset {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}+{}", self.0.x, self.0.y)
	}
}

/// the --tracks-config file: {"video": [...], "audio": [...], "pcm": [...]}, each a list of specs
#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct TracksConfig {
	#[serde(default)] pub video: Vec<VideoSpec>,
	#[serde(default)] pub audio: Vec<AudioSpec>,
	#[serde(default)] pub pcm: Vec<PcmSpec>,
}

impl TracksConfig {
	pub fn read(path: &Path) -> anyhow::Result<Self> {
		let text = std::fs::read_to_string(path).with_context(|| format!("failed to read tracks config '{}'", path.display()))?;
		serde_json::from_str(&text).with_context(|| format!("bad tracks config '{}'", path.display()))
	}
}
