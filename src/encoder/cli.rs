use std::{fmt::Display, num::ParseIntError, ops::Deref, path::{Path, PathBuf}, str::FromStr, time::Duration};
use clap::{Args, Parser, ValueHint};
use num::traits::{ConstOne, ConstZero};
use palette::{Srgb, rgb::channels::Argb};

use crate::{
	EXT, audio::{Mode as AudioMode, dfpwm}, encoder::{
		AudioConfig, EncoderConfig, VideoConfig, VideoDescData
	}, math::{Frac, Point, Rect, Size}, video::{self, cmd::machine::{Machine, Tier}}
};

pub fn process_args(args: Cli) -> EncoderConfig {
	eprintln!("V:{} | A:{}", args.video.is_some(), args.audio.is_some());
	
	let out_path = match args.out_path {
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
	let audio_config = args.audio.as_ref().map(build_audio_config);
	
	EncoderConfig {
		in_path: args.in_path,
		out_path,
		range: args.begin..=args.end,
		machine,
		video: video_config,
		audio: audio_config,
	}
}

fn build_video_config(args: &VideoOpts, machine: &Machine, in_path: &Path) -> VideoConfig {
	let stream_desc = |name: String, size: Size<u8>, source_area: Option<Rect<usize>>| VideoDescData {
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
			let stream_size = args.stream_size.unwrap_or_else(|| {
				let source_size = read_video_size(in_path);
				let max_screen_size = machine.compute_max_resolution(source_size.ratio() / video::braille::SIZE.ratio());
				eprintln!("auto-size: {} (largest possible gpu pixel count & width for ratio)", max_screen_size);
				max_screen_size
			});
			let stream_size: Size<u8> = stream_size.try_cast().expect("stream size too large");

			VideoConfig {
				stream_descs_data: vec![stream_desc("main".to_string(), stream_size, None)],
				container_size: stream_size.cast() * video::braille::SIZE,
				fill_color: *args.fill_color,
			}
		},
		VideoMode::Matrix => {
			let matrix_size = args.matrix_size.unwrap(); //SAFETY: --mode matrix requires --matrix-size
			let stream_size = args.stream_size.unwrap_or_else(|| {
				let size = machine.compute_max_resolution(Frac::ONE / video::braille::SIZE.ratio());
				eprintln!("auto-size: {} (largest possible gpu pixel count & width for ratio)", size);
				size
			});
			let stream_input_size = stream_size * video::braille::SIZE;
			let gap_size = args.matrix_gap_size
				.or_else(|| args.matrix_screen_size
					.map(|matrix_screen_size| {
						let gap = compute_gap_size(stream_input_size, matrix_screen_size);
						eprintln!("auto-gap: {}", gap);
						gap
					}))
				.unwrap_or(Size::ZERO);
			let stream_size: Size<u8> = stream_size.try_cast().expect("stream size too large");

			VideoConfig {
				stream_descs_data: (0..matrix_size.h)
					.flat_map(|y| (0..matrix_size.w).map(move |x| Point::new(x, y)))
					.map(|cell| stream_desc(
						format!("{},{}", cell.x, cell.y),
						stream_size,
						Some(Rect {
							pos: cell * (stream_input_size + gap_size),
							size: stream_input_size,
						}),
					))
					.collect(),
				container_size: matrix_size * stream_input_size + (matrix_size - 1) * gap_size,
				fill_color: *args.fill_color,
			}
		},
		VideoMode::Custom => create_streams_custom(
			args.streams_config.as_ref().unwrap() //SAFETY: --mode custom requires --streams-config
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

pub fn compute_gap_size(stream_size: Size<usize>, matrix_screen_size: Size<usize>) -> Size<usize> {
	const FIXED_POINT: usize = 2;
	const PIXEL_GAP: usize = 9; //it's '(2 + 0.25) * 2' but we use x2 fixed point to remove the floats
	const MINECRAFT_PIXELS: usize = 16;
	//https://www.desmos.com/calculator/balbctweiy
	(stream_size * PIXEL_GAP) / (matrix_screen_size * (MINECRAFT_PIXELS * FIXED_POINT) - PIXEL_GAP)
}

fn create_streams_custom(_config_path: &Path) -> VideoConfig {
	todo!("custom stream config");
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
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum ColorDiffStrategy {
	#[value(name = "fast")] Fast,
	#[value(name = "ciede")] ImprovedCiede2000,
}

pub fn build_audio_config(args: &AudioOpts) -> AudioConfig {
	AudioConfig {
		name: "main".to_string(),
		mode: args.audio_mode,
		pcm_rate: args.pcm_rate,
		analysis_rate: args.analysis_rate,
		fft_window_size: args.fft_window_size,
		hop_length: args.hop_length.unwrap_or(args.fft_window_size / 2),
		guard: args.guard,
		normalize: true,
		num_voices: args.num_voices,
	}
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

	#[command(flatten)]
	audio: Option<AudioOpts>,
}

#[derive(Args, Debug)]
#[group(requires = "mode")]
struct VideoOpts {
	#[arg(
		value_enum,
		short = 'm',
		long = "mode",
		help = "How to arrange and produce the streams",
		requires_ifs([
			("matrix", "matrix_size"),
			("custom", "streams_config"),
		]),
	)]
	pub mode: Option<VideoMode>,

	#[arg(
		short = 'f',
		long = "frame-rate",
		help = "The output framerate, copies input framerate if omitted",
		conflicts_with = "streams_config",
	)]
	pub frame_rate: Option<Frac<u16>>,
	
	#[arg(
		long = "fill-color",
		help = "color of the padding around the video, as RRGGBB hex",
		default_value_t = Srgb::<u8>::from_u32::<Argb>(0x000000).into(),
		conflicts_with = "streams_config",
	)]
	pub fill_color: CliSrgb8,

	#[arg(
		long = "stream-size",
		help = "The size of the streams in characters, fits the GPU limits if omitted",
		conflicts_with = "streams_config",
	)]
	pub stream_size: Option<Size<usize>>,
	
	#[arg(
		long = "matrix-size",
		help = "How many grid cells (streams) to create",
		conflicts_with = "streams_config",
	)]
	pub matrix_size: Option<Size<usize>>,

	#[arg(
		long = "matrix-gap-size",
		help = "sub-pixels to skip between matrix cells, defaults to 0 if omitted",
		conflicts_with = "streams_config",
		conflicts_with = "matrix_screen_size",
	)]
	pub matrix_gap_size: Option<Size<usize>>,
	
	#[arg(
		long = "matrix-screen-size",
		help = "screen size of matrix segments, this is used to derive the matrix gap size",
		conflicts_with = "streams_config",
	)]
	pub matrix_screen_size: Option<Size<usize>>,
	
	#[arg(
		long = "filter",
		help = "what to pass the pixels through before encoding",
		conflicts_with = "streams_config",
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
		conflicts_with = "streams_config",
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
		long = "streams-config",
		help = "path to json streams config file",
	)]
	pub streams_config: Option<PathBuf>,
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
		value_enum,
		long = "audio-mode",
		default_value_t = AudioMode::Voices,
		help = "voices: sound card voices from an fft. pcm: DFPWM for computronics tape drives, play it with play_szt --pcm-tape",
	)]
	pub audio_mode: AudioMode,

	#[arg(
		long = "pcm-rate",
		default_value_t = dfpwm::TAPE_SAMPLE_RATE,
		help = "pcm sample rate (Hz). the tape drive plays 48000 at speed 1, so this sets the speed (12000-96000, a multiple of 32)",
		value_parser = parse_pcm_rate,
	)]
	pub pcm_rate: u32,

	#[arg(
		long = "analysis-rate",
		default_value_t = 24000,
		help = "Target analysis sample rate (Hz)",
	)]
	pub analysis_rate: u32,

	#[arg(
		long = "fft-window-size",
		default_value_t = 1024,
		help = "FFT window size (power of two)",
		value_parser = parse_power_of_two,
	)]
	pub fft_window_size: usize,

	#[arg(
		long = "hop-length",
		help = "Hop size in samples (defaults to window/2)",
	)]
	pub hop_length: Option<usize>,

	#[arg(
		long = "guard",
		default_value_t = 1,
		help = "Bins around a selected peak will be removed",
	)]
	pub guard: isize,

	#[arg(
		short = 'v',
		long = "voices",
		default_value_t = 8,
		help = "How many voices to use (8 channels per sound card)",
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
