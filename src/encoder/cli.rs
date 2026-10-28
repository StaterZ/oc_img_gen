use std::{fmt::Display, num::ParseIntError, ops::Deref, path::{Path, PathBuf}, str::FromStr, time::Duration};
use clap::{Args, Parser, ValueEnum, ValueHint};
use itertools::Itertools;
use num::traits::{ConstOne, ConstZero};
use palette::{Srgb, rgb::channels::Argb};

use crate::{
	EXT, audio::{Mode as AudioMode, dfpwm}, encoder::{
		AudioConfig, EncoderConfig, VideoConfig, VideoDescData,
		track_spec::{AudioSpec, Offset, PcmSpec, TracksConfig, VideoSpec, default_name},
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

	//the tracks from the config file come after the tracks from the flags
	if let Some(path) = &args.tracks_config {
		let config = TracksConfig::read(path)?;
		args.video_specs.extend(config.video);
		args.audio_specs.extend(config.audio);
		args.pcm_specs.extend(config.pcm);
	}

	let machine = args.tier.into();
	let video_config = build_video_config(&args.video, &args.video_specs, &machine, &args.in_path)?;
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

	if let Some(video_config) = video_config {
		lines.push(format!("video: {} tracks on a {} canvas", video_config.track_descs_data.len(), video_config.container_size));
		lines.push(format!("  fill color: {}", CliSrgb8(video_config.fill_color)));
		for desc in &video_config.track_descs_data {
			lines.push(format!("  track: {}", desc.name));
			lines.push(format!("    size: {}", desc.size));
			if let Some(area) = desc.source_area {
				lines.push(format!("    at: {}", Offset(area.pos)));
			}
			lines.push(format!("    frame rate: {}", desc.frame_rate.map_or_else(|| "source".to_string(), |rate| rate.inverse().to_string())));
			lines.push(format!("    filter: {}", desc.filter.as_ref().map_or_else(|| "none".to_string(), value_name)));
			lines.push(format!("    braille: {}", value_name(&desc.braille_strategy)));
			lines.push(format!("    color diff: {}", value_name(&desc.color_diff_strategy)));
			lines.push(format!("    budget: {}", desc.budget.as_ref().map_or_else(|| "none".to_string(), value_name)));
			lines.push(format!("    loss: {}% (step {}%)", desc.acceptable_loss.into_flt::<f64>() * 100.0, desc.loss_step.into_flt::<f64>() * 100.0));
		}
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

/// all video tracks cut their area from one canvas. tracks with `at` (and matrix cells) are placed on it,
/// and the canvas is just large enough to hold them. tracks without `at` show the whole canvas instead
fn build_video_config(args: &VideoOpts, specs: &[VideoSpec], machine: &Machine, in_path: &Path) -> anyhow::Result<Option<VideoConfig>> {
	if specs.is_empty() {
		return Ok(None);
	}

	let track_desc = |spec: &VideoSpec, name: String, size: Size<usize>, source_area: Option<Rect<usize>>| -> anyhow::Result<VideoDescData> {
		Ok(VideoDescData {
			size: size.try_cast().ok_or_else(|| anyhow::anyhow!("video track '{}' is {}, which is too large", name, size))?,
			name,
			frame_rate: spec.frame_rate.or(args.frame_rate).map(|x| x.inverse()),
			source_area,
			filter: spec.filter.or(args.filter),
			braille_strategy: spec.braille.unwrap_or(args.braille_strategy),
			color_diff_strategy: spec.diff.unwrap_or(args.color_diff_strategy),
			budget: spec.budget.or(args.budget),
			acceptable_loss: spec.loss.unwrap_or(args.acceptable_loss),
			loss_step: spec.loss_step.unwrap_or(args.loss_step),
		})
	};

	let mut source_size = None;
	let mut placed = Vec::new();
	let mut unplaced = Vec::new();
	for (index, spec) in specs.iter().enumerate() {
		let track_size = spec.size.or(args.track_size);
		if let Some(matrix_size) = spec.matrix {
			anyhow::ensure!(spec.gap.is_none() || spec.screen.is_none(), "a matrix takes gap or screen, not both");
			let track_size = track_size.unwrap_or_else(|| {
				let size = machine.compute_max_resolution(Frac::ONE / video::braille::SIZE.ratio());
				eprintln!("auto-size: {} (largest possible gpu pixel count & width for ratio)", size);
				size
			});
			let track_input_size = track_size * video::braille::SIZE;
			let gap_size = spec.gap
				.or_else(|| spec.screen
					.map(|matrix_screen_size| {
						let gap = compute_gap_size(track_input_size, matrix_screen_size);
						eprintln!("auto-gap: {}", gap);
						gap
					}))
				.unwrap_or(Size::ZERO);
			let origin = spec.at.map_or(Point::ZERO, |at| at.0);
			for cell in (0..matrix_size.h).flat_map(|y| (0..matrix_size.w).map(move |x| Point::new(x, y))) {
				let name = match &spec.name {
					Some(name) => format!("{}:{},{}", name, cell.x, cell.y),
					None => format!("{},{}", cell.x, cell.y),
				};
				placed.push(track_desc(spec, name, track_size, Some(Rect {
					pos: origin + cell * (track_input_size + gap_size),
					size: track_input_size,
				}))?);
			}
		} else {
			anyhow::ensure!(spec.gap.is_none() && spec.screen.is_none(), "gap and screen only work with matrix");
			let track_size = track_size.unwrap_or_else(|| {
				let source_size = *source_size.get_or_insert_with(|| read_video_size(in_path));
				let max_screen_size = machine.compute_max_resolution(source_size.ratio() / video::braille::SIZE.ratio());
				eprintln!("auto-size: {} (largest possible gpu pixel count & width for ratio)", max_screen_size);
				max_screen_size
			});
			let name = default_name(spec.name.clone(), index, specs.len());
			match spec.at {
				Some(at) => placed.push(track_desc(spec, name, track_size, Some(Rect {
					pos: at.0,
					size: track_size * video::braille::SIZE,
				}))?),
				None => unplaced.push(track_desc(spec, name, track_size, None)?),
			}
		}
	}

	if let Some(name) = placed.iter().chain(unplaced.iter()).map(|desc| &desc.name).duplicates().next() {
		anyhow::bail!("there are two video tracks named '{}'", name);
	}

	let container_size = match (placed.is_empty(), unplaced.as_slice()) {
		(true, [first, rest @ ..]) => {
			if let Some(other) = rest.iter().find(|desc| desc.size != first.size) {
				anyhow::bail!("video tracks without `at` show the whole canvas, so they need the same size, but '{}' is {} and '{}' is {}", first.name, first.size, other.name, other.size);
			}
			first.size.cast() * video::braille::SIZE
		},
		(false, []) => placed
			.iter()
			.filter_map(|desc| desc.source_area)
			.fold(Size::ZERO, |size: Size<usize>, area| Size::new(
				size.w.max(area.pos.x + area.size.w),
				size.h.max(area.pos.y + area.size.h),
			)),
		_ => anyhow::bail!("video tracks without `at` show the whole canvas, so they can not share it with tracks that have `at` or `matrix`"),
	};

	Ok(Some(VideoConfig {
		track_descs_data: placed.into_iter().chain(unplaced).collect(),
		container_size,
		fill_color: *args.fill_color,
	}))
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

	#[arg(
		long = "video",
		value_name = "SPEC",
		num_args = 0..=1,
		default_missing_value = "",
		help = "Add a video track. SPEC is key=value pairs split by commas, the keys are: name, size, at (X+Y on the canvas, in sub-pixels), matrix (WxH tracks), gap, screen, frame-rate, filter, braille, diff, budget, loss, loss-step",
	)]
	video_specs: Vec<VideoSpec>,

	#[arg(
		long = "tracks-config",
		help = "Path to a json file with more tracks: {\"video\": [...], \"audio\": [...], \"pcm\": [...]}. each track is an object with the same keys as the SPEC of its flag",
		value_hint = ValueHint::FilePath,
	)]
	tracks_config: Option<PathBuf>,

	#[command(flatten)]
	video: VideoOpts,

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
struct VideoOpts {
	#[arg(
		short = 'f',
		long = "frame-rate",
		help = "Default output framerate, copies input framerate if omitted",
	)]
	pub frame_rate: Option<Frac<u16>>,
	
	#[arg(
		long = "fill-color",
		help = "color of the padding around the video, as RRGGBB hex",
		default_value_t = Srgb::<u8>::from_u32::<Argb>(0x000000).into(),
	)]
	pub fill_color: CliSrgb8,

	#[arg(
		long = "track-size",
		help = "Default size of the tracks in characters, fits the GPU limits if omitted",
	)]
	pub track_size: Option<Size<usize>>,

	#[arg(
		long = "filter",
		help = "Default for what to pass the pixels through before encoding",
	)]
	pub filter: Option<VideoFilter>,

	#[arg(
		value_enum,
		short = 'B',
		long = "braille",
		help = "Default brailling strategy",
		default_value_t = BrailleStrategy::CentroidCohesion,
	)]
	braille_strategy: BrailleStrategy,

	#[arg(
		value_enum,
		short = 'D',
		long = "diff",
		help = "Default color comparison strategy",
		default_value_t = ColorDiffStrategy::Fast,
	)]
	color_diff_strategy: ColorDiffStrategy,

	#[arg(
		short = 'b',
		long = "budget",
		help = "Default budget. when enabled, the video will lower in framerate if the frame complexity becomes higher than the GPU render budget",
	)]
	pub budget: Option<Budget>,
	
	
	#[arg(
		short = 'l',
		long = "loss",
		help = "Default highest loss a frame may have when it's over budget",
		requires = "budget",
		default_value = "0",
	)]
	pub acceptable_loss: Frac<u32>,

	#[arg(
		long = "loss-step",
		help = "Default for how much to raise the loss each time a frame is over budget",
		requires = "loss",
		default_value = "1%",
	)]
	pub loss_step: Frac<u32>,
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
