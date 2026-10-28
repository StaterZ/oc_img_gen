//! the tracks to encode, one spec per track. a spec comes from a cli flag as "key=value,key=value",
//! or from the tracks config file with the same keys. keys that are left out use the global flags

use std::{fmt::Display, str::FromStr};
use clap::ValueEnum;
use serde::{Deserialize, Deserializer, de::{DeserializeOwned, Error}};

/// parses "key=value,key=value". an empty string is a spec with only defaults
pub fn parse_spec<T: DeserializeOwned>(s: &str) -> Result<T, String> {
	let mut map = serde_json::Map::new();
	for pair in s.split(',').map(str::trim).filter(|pair| !pair.is_empty()) {
		let (key, value) = pair.split_once('=').ok_or_else(|| format!("'{pair}' is not key=value"))?;
		let key = key.trim();
		if map.insert(key.to_string(), serde_json::Value::String(value.trim().to_string())).is_some() {
			return Err(format!("'{key}' is set more than once"));
		}
	}
	serde_json::from_value(map.into()).map_err(|e| e.to_string())
}

macro_rules! impl_from_str {
	($($spec:ty),*) => {$(
		impl FromStr for $spec {
			type Err = String;

			fn from_str(s: &str) -> Result<Self, Self::Err> {
				parse_spec(s)
			}
		}
	)*};
}

/// the cli gives every value as text, the config file can also give numbers
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

impl_from_str!(AudioSpec, PcmSpec);

/// the name a track gets when its spec has none. a lone track of its kind is "main"
pub fn default_name(name: Option<String>, index: usize, count: usize) -> String {
	name.unwrap_or_else(|| if count == 1 { "main".to_string() } else { index.to_string() })
}
