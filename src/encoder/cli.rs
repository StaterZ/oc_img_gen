use std::{fmt::Display, num::ParseIntError, ops::Deref, path::{Path, PathBuf}, str::FromStr, time::Duration};
use clap::{Args, Parser, ValueEnum, ValueHint};
use itertools::Itertools;
use num::traits::{ConstOne, ConstZero};
use palette::{Srgb, rgb::channels::Argb};

use crate::{
	EXT, audio::{Mode as AudioMode, dfpwm}, encoder::{
		AudioConfig, EncoderConfig, VideoConfig, VideoDescData,
		track_spec::{AudioSpec, PcmSpec, default_name},
	}, math::{Frac, Point, Rect, Size}, video::{self, cmd::machine::{Machine, Tier}}
};

pub fn process_args(mut args: Cli) -> anyhow::Result<EncoderConfig> {
	
	let out_path = match args.out_path.take() {
		None => {
			let mut path = args.in_path.clone();
			path.set_extension(EXT);
			path
		},
		Some(mut path) => {
			if path.is_dir() {
				path.push(args.in_path.file_name().expect("Bad input path"));
				path.set_extension(EXT);
			} else if path.extension() == None {
				path.set_extension(EXT);
			}
			path
		}
	};

	let machine = args.tier.into();
	let video_config = args.video.as_ref().map(|video_args| build_video_config(video_args, &machine, &args.in_path));
	let audio_configs = build_audio_configs(&args.audio, &args.audio_specs, &args.pcm_specs)?;
	if video_config.is_none() && audio_configs.is_empty() {
		anyhow::bail!("there are no tracks to encode. add one with --video, --audio or --pcm");
	}
	eprintln!("V:{} | A:{}", video_config.as_ref().map_or(0, |video_config| video_config.track_descs_data.len()), audio_configs.len());
	let settings = describe_settings(&args, video_config.as_ref(), &audio_configs);
	
	Ok(EncoderConfig {
		settings,
		in_path: args.in_path,
		out_path,
		range: args.begin..=args.end,
		machine,
		video: video_config,
		audio: audio_configs,
	})
}

fn value_name(value: &impl ValueEnum) -> String {
	value.to_possible_value().map_or_else(String::new, |value| value.get_name().to_string())
}

/// the settings the file was encoded with, as "key: value" lines. nested settings are indented
fn describe_settings(args: &Cli, video_config: Option<&VideoConfig>, audio_configs: &[AudioConfig]) -> String {
	let time = |time: Option<Duration>, fallback: &str| time.map_or_else(|| fallback.to_string(), |time| humantime::Duration::from(time).to_string());
	let mut lines = vec![
		format!("encoder: {} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
		format!("encoded: {}", humantime::format_rfc3339_seconds(std::time::SystemTime::now())),
		format!("input: {}", args.in_path.file_name().map_or_else(|| args.in_path.display().to_string(), |name| name.to_string_lossy().to_string())),
		format!("range: {} to {}", time(args.begin, "start"), time(args.end, "end")),
		format!("tier: {}", value_name(&args.tier)),
	];

	if let (Some(video), Some(video_config)) = (&args.video, video_config) {
		lines.push(format!("video: {}", video.mode.as_ref().map_or_else(String::new, value_name)));
		if let Some(desc) = video_config.track_descs_data.first() {
			lines.push(format!("  tracks: {} of {}", video_config.track_descs_data.len(), desc.size));
		}
		lines.push(format!("  frame rate: {}", video.frame_rate.map_or_else(|| "source".to_string(), |rate| rate.to_string())));
		lines.push(format!("  fill color: {}", video.fill_color));
		if let Some(gap) = video.matrix_gap_size {
			lines.push(format!("  matrix gap: {}", gap));
		}
		if let Some(screen_size) = video.matrix_screen_size {
			lines.push(format!("  matrix screen size: {}", screen_size));
		}
		lines.push(format!("  filter: {}", video.filter.as_ref().map_or_else(|| "none".to_string(), value_name)));
		lines.push(format!("  braille: {}", value_name(&video.braille_strategy)));
		lines.push(format!("  color diff: {}", value_name(&video.color_diff_strategy)));
		lines.push(format!("  budget: {}", video.budget.as_ref().map_or_else(|| "none".to_string(), value_name)));
		lines.push(format!("  loss: {}% (step {}%)", video.acceptable_loss.into_flt::<f64>() * 100.0, video.loss_step.into_flt::<f64>() * 100.0));
	}

	for audio in audio_configs {
		lines.push(format!("audio: {}", audio.name));
		lines.push(format!("  mode: {}", value_name(&audio.mode)));
		match audio.mode {
			AudioMode::Voices => {
				lines.push(format!("  voices: {}", audio.num_voices));
				lines.push(format!("  analysis rate: {}Hz", audio.analysis_rate));
				lines.push(format!("  fft window: {}", audio.fft_window_size));
				lines.push(format!("  hop length: {}", audio.hop_length));
				lines.push(format!("  guard: {}", audio.guard));
			},
			AudioMode::Pcm => {
				lines.push(format!("  sample rate: {}Hz (tape speed {:.2})", audio.pcm_rate, dfpwm::tape_speed(audio.pcm_rate)));
				lines.push(format!("  stream align: {} bytes", audio.pcm_align));
			},
		}
	}
	lines.join("\n")
}

fn build_video_config(args: &VideoOpts, machine: &Machine, in_path: &Path) -> VideoConfig {
	let track_desc = |name: String, size: Size<u8>, source_area: Option<Rect<usize>>| VideoDescData {
		name,
		frame_rate: args.frame_rate.map(|x| x.inverse()),
		size,
		source_area,
		filter: args.filter,
		braille_strategy: args.braille_strategy,
		color_diff_strategy: args.color_diff_strategy,
		budget: args.budget,
		acceptable_loss: args.acceptable_loss,
		loss_step: args.loss_step,
	};

	match args.mode.unwrap() { //SAFETY: the video option group requires --mode
		VideoMode::Single => {
			let track_size = args.track_size.unwrap_or_else(|| {
				let source_size = read_video_size(in_path);
				let max_screen_size = machine.compute_max_resolution(source_size.ratio() / video::braille::SIZE.ratio());
				eprintln!("auto-size: {} (largest possible gpu pixel count & width for ratio)", max_screen_size);
				max_screen_size
			});
			let track_size: Size<u8> = track_size.try_cast().expect("track size too large");

			VideoConfig {
				track_descs_data: vec![track_desc("main".to_string(), track_size, None)],
				container_size: track_size.cast() * video::braille::SIZE,
				fill_color: *args.fill_color,
			}
		},
		VideoMode::Matrix => {
			let matrix_size = args.matrix_size.unwrap(); //SAFETY: --mode matrix requires --matrix-size
			let track_size = args.track_size.unwrap_or_else(|| {
				let size = machine.compute_max_resolution(Frac::ONE / video::braille::SIZE.ratio());
				eprintln!("auto-size: {} (largest possible gpu pixel count & width for ratio)", size);
				size
			});
			let track_input_size = track_size * video::braille::SIZE;
			let gap_size = args.matrix_gap_size
				.or_else(|| args.matrix_screen_size
					.map(|matrix_screen_size| {
						let gap = compute_gap_size(track_input_size, matrix_screen_size);
						eprintln!("auto-gap: {}", gap);
						gap
					}))
				.unwrap_or(Size::ZERO);
			let track_size: Size<u8> = track_size.try_cast().expect("track size too large");

			VideoConfig {
				track_descs_data: (0..matrix_size.h)
					.flat_map(|y| (0..matrix_size.w).map(move |x| Point::new(x, y)))
					.map(|cell| track_desc(
						format!("{},{}", cell.x, cell.y),
						track_size,
						Some(Rect {
							pos: cell * (track_input_size + gap_size),
							size: track_input_size,
						}),
					))
					.collect(),
				container_size: matrix_size * track_input_size + (matrix_size - 1) * gap_size,
				fill_color: *args.fill_color,
			}
		},
		VideoMode::Custom => create_tracks_custom(
			args.tracks_config.as_ref().unwrap() //SAFETY: --mode custom requires --tracks-config
		),
	}
}

fn read_video_size(in_path: &Path) -> Size<usize> {
	let ictx = ffmpeg_next::format::input(in_path).expect("failed to create decoder");
	let stream = ictx.streams().best(ffmpeg_next::media::Type::Video).unwrap();
	let codec_ctx = ffmpeg_next::codec::Context::from_parameters(stream.parameters()).expect("failed to create codec context");
	let decoder = codec_ctx.decoder().video().unwrap();
	Size::new(decoder.width() as usize, decoder.height() as usize)
}

pub fn compute_gap_size(track_size: Size<usize>, matrix_screen_size: Size<usize>) -> Size<usize> {
	const FIXED_POINT: usize = 2;
	const PIXEL_GAP: usize = 9; //it's '(2 + 0.25) * 2' but we use x2 fixed point to remove the floats
	const MINECRAFT_PIXELS: usize = 16;
	//https://www.desmos.com/calculator/balbctweiy
	(track_size * PIXEL_GAP) / (matrix_screen_size * (MINECRAFT_PIXELS * FIXED_POINT) - PIXEL_GAP)
}

fn create_tracks_custom(_config_path: &Path) -> VideoConfig {
	todo!("custom track config");
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum VideoMode {
	Single,
	Matrix,
	Custom,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum VideoFilter {
	Monochrome,
	Grayscale,
	Vga,
	Hsv,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum Budget {
	Direct,
	Buffered,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum BrailleStrategy {
	CentroidCohesion,
	PolarPair,
	PalettePair,
	PaletteDither,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum ColorDiffStrategy {
	#[value(name = "fast")] Fast,
	#[value(name = "ciede")] ImprovedCiede2000,
}

/// the voices tracks, then the pcm tracks, each in the order of their flags
fn build_audio_configs(args: &AudioOpts, audio_specs: &[AudioSpec], pcm_specs: &[PcmSpec]) -> anyhow::Result<Vec<AudioConfig>> {
	let config = |name: String, mode: AudioMode| AudioConfig {
		name,
		mode,
		pcm_rate: args.pcm_rate,
		pcm_align: args.pcm_align,
		analysis_rate: args.analysis_rate,
		fft_window_size: args.fft_window_size,
		hop_length: args.hop_length.unwrap_or(args.fft_window_size / 2),
		guard: args.guard,
		normalize: true,
		num_voices: args.num_voices,
	};

	let mut configs = Vec::new();
	for (index, spec) in audio_specs.iter().enumerate() {
		let mut config = config(default_name(spec.name.clone(), index, audio_specs.len()), AudioMode::Voices);
		if let Some(num_voices) = spec.voices {
			anyhow::ensure!(num_voices >= 1, "audio track '{}' needs at least 1 voice", config.name);
			config.num_voices = num_voices;
		}
		config.analysis_rate = spec.analysis_rate.unwrap_or(config.analysis_rate);
		if let Some(fft_window_size) = spec.fft_window_size {
			config.fft_window_size = parse_power_of_two(&fft_window_size.to_string()).map_err(anyhow::Error::msg)?;
			config.hop_length = args.hop_length.unwrap_or(fft_window_size / 2);
		}
		config.hop_length = spec.hop_length.unwrap_or(config.hop_length);
		config.guard = spec.guard.unwrap_or(config.guard);
		configs.push(config);
	}
	for (index, spec) in pcm_specs.iter().enumerate() {
		let mut config = config(default_name(spec.name.clone(), index, pcm_specs.len()), AudioMode::Pcm);
		if let Some(pcm_rate) = spec.rate {
			config.pcm_rate = parse_pcm_rate(&pcm_rate.to_string()).map_err(anyhow::Error::msg)?;
		}
		if let Some(pcm_align) = spec.align {
			anyhow::ensure!(pcm_align >= 1, "pcm track '{}' needs an align of at least 1", config.name);
			config.pcm_align = pcm_align;
		}
		configs.push(config);
	}

	if let Some((mode, name)) = configs.iter().map(|config| (config.mode, &config.name)).duplicates().next() {
		anyhow::bail!("there are two {} tracks named '{}'", value_name(&mode), name);
	}
	Ok(configs)
}

#[derive(Parser, Debug)]
pub struct Cli {
	#[arg(
		short = 'i',
		long = "in",
		help = "Path to image or video to encode",
		value_hint = ValueHint::FilePath,
	)]
	pub in_path: PathBuf,

	#[arg(
		short = 'o',
		long = "out",
		help = "Path to the output SZT file or folder, saves next to the input with .szt extension if omitted",
		value_hint = ValueHint::FilePath,
	)]
	pub out_path: Option<PathBuf>,


	#[arg(
		short = 's',
		long = "start",
		help = "Time to start from, starts at input start if omitted",
		value_parser = humantime::parse_duration,
	)]
	pub begin: Option<Duration>,

	#[arg(
		short = 'e',
		long = "end",
		help = "Time to stop at (inclusive), stops at input end if omitted",
		value_parser = humantime::parse_duration,
	)]
	pub end: Option<Duration>,

	#[arg(
		value_enum,
		short = 't',
		long = "tier",
		help = "What machine tier to optimize for",
		default_value_t = Tier::T3,
	)]
	tier: Tier,

	#[command(flatten)]
	video: Option<VideoOpts>,

	#[arg(
		long = "audio",
		value_name = "SPEC",
		num_args = 0..=1,
		default_missing_value = "",
		help = "Add a sound card voices track. SPEC is key=value pairs split by commas, the keys are: name, voices, analysis-rate, fft-window-size, hop-length, guard",
	)]
	audio_specs: Vec<AudioSpec>,

	#[arg(
		long = "pcm",
		value_name = "SPEC",
		num_args = 0..=1,
		default_missing_value = "",
		help = "Add a DFPWM track for computronics tape drives, play it with play_szt --pcm-tape. SPEC is key=value pairs split by commas, the keys are: name, rate, align",
	)]
	pcm_specs: Vec<PcmSpec>,

	#[command(flatten)]
	audio: AudioOpts,
}

#[derive(Args, Debug)]
#[group(requires = "mode")]
struct VideoOpts {
	#[arg(
		value_enum,
		short = 'm',
		long = "mode",
		help = "How to arrange and produce the tracks",
		requires_ifs([
			("matrix", "matrix_size"),
			("custom", "tracks_config"),
		]),
	)]
	pub mode: Option<VideoMode>,

	#[arg(
		short = 'f',
		long = "frame-rate",
		help = "The output framerate, copies input framerate if omitted",
		conflicts_with = "tracks_config",
	)]
	pub frame_rate: Option<Frac<u16>>,
	
	#[arg(
		long = "fill-color",
		help = "color of the padding around the video, as RRGGBB hex",
		default_value_t = Srgb::<u8>::from_u32::<Argb>(0x000000).into(),
		conflicts_with = "tracks_config",
	)]
	pub fill_color: CliSrgb8,

	#[arg(
		long = "track-size",
		help = "The size of the tracks in characters, fits the GPU limits if omitted",
		conflicts_with = "tracks_config",
	)]
	pub track_size: Option<Size<usize>>,
	
	#[arg(
		long = "matrix-size",
		help = "How many grid cells (tracks) to create",
		conflicts_with = "tracks_config",
	)]
	pub matrix_size: Option<Size<usize>>,

	#[arg(
		long = "matrix-gap-size",
		help = "sub-pixels to skip between matrix cells, defaults to 0 if omitted",
		conflicts_with = "tracks_config",
		conflicts_with = "matrix_screen_size",
	)]
	pub matrix_gap_size: Option<Size<usize>>,
	
	#[arg(
		long = "matrix-screen-size",
		help = "screen size of matrix segments, this is used to derive the matrix gap size",
		conflicts_with = "tracks_config",
	)]
	pub matrix_screen_size: Option<Size<usize>>,
	
	#[arg(
		long = "filter",
		help = "what to pass the pixels through before encoding",
		conflicts_with = "tracks_config",
	)]
	pub filter: Option<VideoFilter>,

	#[arg(
		value_enum,
		short = 'B',
		long = "braille",
		help = "What brailling strategy to use",
		default_value_t = BrailleStrategy::CentroidCohesion,
	)]
	braille_strategy: BrailleStrategy,

	#[arg(
		value_enum,
		short = 'D',
		long = "diff",
		help = "What color comparison strategy to use",
		default_value_t = ColorDiffStrategy::Fast,
	)]
	color_diff_strategy: ColorDiffStrategy,

	#[arg(
		short = 'b',
		long = "budget",
		help = "when enabled, the video will lower in framerate if the frame complexity becomes higher than the GPU render budget",
		conflicts_with = "tracks_config",
	)]
	pub budget: Option<Budget>,
	
	
	#[arg(
		short = 'l',
		long = "loss",
		help = "highest loss a frame may have when it's over budget",
		default_value = "0",
	)]
	pub acceptable_loss: Frac<u32>,

	#[arg(
		long = "loss-step",
		help = "how much to raise the loss each time a frame is over budget",
		default_value = "1%",
	)]
	pub loss_step: Frac<u32>,
	
	#[arg(
		long = "tracks-config",
		help = "path to json tracks config file",
	)]
	pub tracks_config: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct CliSrgb8(Srgb<u8>);

#[derive(thiserror::Error, Debug)]
pub enum ParseColorError {
	#[error("Color must be 6 hex digits, {0} supplied")]
	BadCharacterCount(usize),
	#[error("{0}")]
	ParseIntError(#[from] ParseIntError),
}

impl FromStr for CliSrgb8 {
	type Err = ParseColorError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let hex = s.trim();
		if hex.len() != 6 {
			return Err(ParseColorError::BadCharacterCount(hex.len()));
		}
		let r = u8::from_str_radix(&hex[0..2], 16)?;
		let g = u8::from_str_radix(&hex[2..4], 16)?;
		let b = u8::from_str_radix(&hex[4..6], 16)?;
		Ok(CliSrgb8(Srgb::new(r, g, b)))
	}
}

impl Display for CliSrgb8 {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
	}
}

impl Deref for CliSrgb8 {
	type Target = Srgb::<u8>;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl From<Srgb<u8>> for CliSrgb8 {
	fn from(value: Srgb<u8>) -> Self {
		Self(value)
	}
}

#[derive(Args, Debug)]
pub struct AudioOpts {
	#[arg(
		long = "pcm-rate",
		default_value_t = dfpwm::TAPE_SAMPLE_RATE,
		help = "Default pcm sample rate (Hz). the tape drive plays 48000 at speed 1, so this sets the speed (12000-96000, a multiple of 32)",
		value_parser = parse_pcm_rate,
	)]
	pub pcm_rate: u32,

	#[arg(
		long = "pcm-align",
		default_value_t = 4096,
		help = "Default align: each pcm track gets its own stream, which starts on a multiple of this many bytes",
		value_parser = clap::value_parser!(u32).range(1..),
	)]
	pub pcm_align: u32,

	#[arg(
		long = "analysis-rate",
		default_value_t = 24000,
		help = "Default target analysis sample rate (Hz)",
	)]
	pub analysis_rate: u32,

	#[arg(
		long = "fft-window-size",
		default_value_t = 1024,
		help = "Default FFT window size (power of two)",
		value_parser = parse_power_of_two,
	)]
	pub fft_window_size: usize,

	#[arg(
		long = "hop-length",
		help = "Default hop size in samples (defaults to window/2)",
	)]
	pub hop_length: Option<usize>,

	#[arg(
		long = "guard",
		default_value_t = 1,
		help = "Default bins around a selected peak that will be removed",
	)]
	pub guard: isize,

	#[arg(
		short = 'v',
		long = "voices",
		default_value_t = 8,
		help = "Default number of voices (8 channels per sound card)",
		value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..),
	)]
	pub num_voices: usize,
}

fn parse_pcm_rate(s: &str) -> Result<u32, String> {
	let value = s.parse::<u32>().map_err(|e| e.to_string())?;
	let speed = dfpwm::tape_speed(value);
	if !(dfpwm::TAPE_MIN_SPEED..=dfpwm::TAPE_MAX_SPEED).contains(&speed) {
		return Err(format!("{value}Hz needs tape speed {speed:.2}, but tape drives only play at {}-{}", dfpwm::TAPE_MIN_SPEED, dfpwm::TAPE_MAX_SPEED));
	}
	if value % dfpwm::TAPE_SAMPLE_RATE_STEP != 0 {
		return Err(format!("{value}Hz is not a multiple of {}, which is the step the tape drive speed has", dfpwm::TAPE_SAMPLE_RATE_STEP));
	}
	Ok(value)
}

fn parse_power_of_two(s: &str) -> Result<usize, String> {
	let value = s.parse::<usize>().map_err(|e| e.to_string())?;
	if !value.is_power_of_two() {
		return Err(format!("{value} is not a power of two"));
	}
	Ok(value)
}
