use std::{path::{Path, PathBuf}, time::Duration};
use clap::{Args, Parser, ValueHint};
use num_traits::{ConstOne, ConstZero};

use crate::{
	EXT, encoder::{
		AudioConfig, EncoderConfig, VideoConfig, VideoDescData
	}, math::{Frac, Point, Rect, Size, SizeTrait}, video::{self, cmd::machine::{Machine, Tier}, oc_color::RGB8}
};

pub fn process_args(args: Cli) -> EncoderConfig {
	if !args.validate() {
		std::process::exit(1);
	}
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

	let video_config = args.video.as_ref().map(|video_args| {
		let ictx = ffmpeg_next::format::input(&args.in_path).expect("failed to create decoder");
		let stream = ictx.streams().best(ffmpeg_next::media::Type::Video).unwrap();
		let codec_ctx = ffmpeg_next::codec::Context::from_parameters(stream.parameters()).expect("failed to create codec context");
		let decoder = codec_ctx.decoder().video().unwrap();
		
		build_video_config(video_args, &machine, &decoder)
	});
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

fn build_video_config(args: &VideoOpts, machine: &Machine, stream: &ffmpeg_next::decoder::Video) -> VideoConfig {
	match args.mode.unwrap() { //SAFETY: unwrap safe due to argument validation in caller
		VideoMode::Single => {
			let stream_size = args.stream_size.unwrap_or_else(|| {
				let source_size = Size::<u32>::new(stream.width(), stream.height());
				let max_screen_size = machine.compute_max_resolution(source_size.cast::<usize>().ratio() / video::braille::SIZE.ratio());
				eprintln!("auto-size: {} (largest possible gpu pixel count & width for ratio)", max_screen_size);
				max_screen_size
			});

			create_main_stream(
				args.frame_rate.map(|x| x.inverse()),
				args.fill_color,
				stream_size.try_cast().expect("stream size too large"),
				args.filter,
				args.braille_strategy.unwrap_or(BrailleStrategy::CentroidCohesion),
				args.budget,
				args.acceptable_loss.unwrap_or(Frac::from(0)),
			)
		},
		VideoMode::Matrix => {
			let stream_size = args.stream_size.unwrap_or_else(|| {
				let size = machine.compute_max_resolution(Frac::ONE / video::braille::SIZE.ratio());
				eprintln!("auto-size: {} (largest possible gpu pixel count & width for ratio)", size);
				size
			});
			
			let gap_size = args.matrix_gap_size
				.or_else(|| args.matrix_screen_size
					.map(|matrix_screen_size| {
						let gap = compute_gap_size(stream_size * video::braille::SIZE, matrix_screen_size);
						eprintln!("auto-gap: {}", gap);
						gap
					}))
				.unwrap_or(Size::ZERO);
			
			create_matrix_streams(
				args.frame_rate.map(|x| x.inverse()),
				args.fill_color,
				stream_size.try_cast().expect("stream size too large"),
				args.matrix_size.unwrap(),
				gap_size,
				args.filter,
				args.braille_strategy.unwrap_or(BrailleStrategy::CentroidCohesion),
				args.budget,
				args.acceptable_loss.unwrap_or(Frac::from(0)),
			)
		},
		VideoMode::Custom => create_streams_custom(
			args.streams_config.as_ref().unwrap()
		),
	}
}

pub fn compute_gap_size(stream_size: Size<usize>, matrix_screen_size: Size<usize>) -> Size<usize> {
	const FIXED_POINT: usize = 2;
	const PIXEL_GAP: usize = 9; //it's '(2 + 0.25) * 2' but we use x2 fixed point to remove the floats
	const MINECRAFT_PIXELS: usize = 16;
	//https://www.desmos.com/calculator/balbctweiy
	(stream_size * PIXEL_GAP) / (matrix_screen_size * (MINECRAFT_PIXELS * FIXED_POINT) - PIXEL_GAP)
}

fn create_main_stream(
	frame_rate: Option<Frac<u16>>,
	fill_color: RGB8,
	stream_size: Size<u8>,
	filter: Option<VideoFilter>,
	braille_strategy: BrailleStrategy,
	budget: Option<Budget>,
	acceptable_loss: Frac<u64>,
) -> VideoConfig {
	let stream_descs_data = vec![VideoDescData {
		name: "main".to_string(),
		frame_rate,
		size: stream_size,
		source_area: None,
		filter,
		braille_strategy,
		budget,
		acceptable_loss,
	}];

	VideoConfig {
		stream_descs_data,
		container_size: stream_size.cast() * video::braille::SIZE,
		fill_color,
	}
}

fn create_matrix_streams(
	frame_rate: Option<Frac<u16>>,
	fill_color: RGB8,
	stream_size: Size<u8>,
	matrix_size: Size<usize>,
	matrix_gap_size: Size<usize>,
	filter: Option<VideoFilter>,
	braille_strategy: BrailleStrategy,
	budget: Option<Budget>,
	acceptable_loss: Frac<u64>,
) -> VideoConfig {
	let stream_input_size = stream_size.cast() * video::braille::SIZE;
	let container_size = matrix_size * stream_input_size + (matrix_size - 1) * matrix_gap_size;

	let stream_descs_data = (0..matrix_size.h)
		.flat_map(move |y| (0..matrix_size.w)
			.map(move |x| VideoDescData {
				name: format!("{},{}", x, y),
				frame_rate,
				size: stream_size,
				source_area: Some(Rect {
					pos: Point::new(x, y) * (stream_input_size + matrix_gap_size),
					size: stream_input_size,
				}),
				filter,
				braille_strategy,
				budget,
				acceptable_loss,
			}))
		.collect();

	VideoConfig {
		stream_descs_data,
		container_size,
		fill_color,
	}
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
	AxisSplit,
}

pub fn build_audio_config(args: &AudioOpts) -> AudioConfig {
	AudioConfig {
		name: "main".to_string(),
		analysis_rate: args.analysis_rate,
		fft_window_size: args.fft_window_size,
		hop_length: args.hop_length.unwrap_or(args.fft_window_size / 2),
		guard: args.guard,
		normalize: true,
		num_voices: args.num_voices,
	}
}

#[derive(Parser, Debug)]
#[command(author, version)]
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
		help = "Path to where to the output SZT file, saves at in_path with .szt extension if omitted",
		value_hint = ValueHint::FilePath,
	)]
	pub out_path: Option<PathBuf>,


	#[arg(
		short = 's',
		long = "start",
		help = "What frame to start from, starts at input start if omitted",
		value_parser = humantime::parse_duration,
	)]
	pub begin: Option<Duration>,

	#[arg(
		short = 'e',
		long = "end",
		help = "What frame to stop from (inclusive), stops at input end if omitted",
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

impl Cli {
	fn validate(&self) -> bool {
		let mut is_valid = true;
		if let Some(video) = self.video.as_ref() {
			is_valid &= video.validate();
		}
		if let Some(audio) = self.audio.as_ref() {
			is_valid &= audio.validate();
		}
		is_valid
	}
}

#[derive(Args, Debug)]
struct VideoOpts {
	#[arg(
		value_enum,
		short = 'm',
		long = "mode",
		help = "How to arrange and produce the streams",
		requires_ifs([
			("matrix", "matrix_size"),
			("matrix", "matrix_gap_size"),
			("matrix", "matrix_screen_size"),
			("custom", "streams_config"),
		]),
	)]
	pub mode: Option<VideoMode>,

	#[arg(
		short = 'f',
		long = "frame-rate",
		help = "The output framerate, copies input framerate if omitted",
		requires = "mode",
		conflicts_with = "streams_config",
	)]
	pub frame_rate: Option<Frac<u16>>,
	
	#[arg(
		long = "fill-color",
		help = "color of the padding around the video",
		default_value_t = RGB8::new(0x000000),
		requires = "mode",
		conflicts_with = "streams_config",
	)]
	pub fill_color: RGB8,

	#[arg(
		long = "stream-size",
		help = "The size of the streams",
		requires = "mode",
		conflicts_with = "streams_config",
	)]
	pub stream_size: Option<Size<usize>>,
	
	#[arg(
		long = "matrix-size",
		help = "How many grid cells (streams) to create",
		requires = "mode",
		required_if_eq("mode", "Matrix"),
		conflicts_with = "streams_config",
	)]
	pub matrix_size: Option<Size<usize>>,

	#[arg(
		long = "matrix-gap-size",
		help = "sub-pixels to skip between matrix cells, defaults to 0 to omitted",
		requires = "mode",
		required_if_eq("mode", "Matrix"),
		conflicts_with = "streams_config",
		conflicts_with = "matrix_screen_size",
	)]
	pub matrix_gap_size: Option<Size<usize>>,
	
	#[arg(
		long = "matrix-screen-size",
		help = "screen size of matrix segments, this is used to derive the matrix gap size",
		requires = "mode",
		required_if_eq("mode", "Matrix"),
		conflicts_with = "streams_config",
		conflicts_with = "matrix_gap_size",
	)]
	pub matrix_screen_size: Option<Size<usize>>,
	
	#[arg(
		long = "filter",
		help = "what to pass the pixels through before encoding",
		requires = "mode",
		conflicts_with = "streams_config",
	)]
	pub filter: Option<VideoFilter>,

	#[arg(
		value_enum,
		short = 'b',
		long = "braille",
		help = "What brailling strategy to use",
		requires = "mode",
		//default_value_t = Some(BrailleStrategy::Soft), //TODO: Problems?
	)]
	braille_strategy: Option<BrailleStrategy>,

	#[arg(
		long = "budget",
		help = "when enabled, the video will lower in framerate if the frame complexity becomes higher than the GPU render budget",
		requires = "mode",
		conflicts_with = "streams_config",
	)]
	pub budget: Option<Budget>,
	
	
	#[arg(
		long = "loss",
		help = "selects the 'bitrate' limit for each frame. default: 0",
	)]
	pub acceptable_loss: Option<Frac<u64>>,
	
	#[arg(
		long = "streams-config",
		help = "path to json streams config file",
		requires = "mode",
	)]
	pub streams_config: Option<PathBuf>,
}

impl VideoOpts {
	fn validate(&self) -> bool {
		true
	}
}

#[derive(Args, Debug)]
pub struct AudioOpts {
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
		long = "voices",
		default_value_t = 8,
		help = "How many voices to use (8 channels per sound card)",
	)]
	pub num_voices: usize,
}
impl AudioOpts {
	fn validate(&self) -> bool {
		let mut is_valid = true;
		if !self.fft_window_size.is_power_of_two() {
			eprintln!("--window must be a power of two");
			is_valid = false;
		}
		if self.num_voices == 0 {
			eprintln!("--you need at least one voice");
			is_valid = false;
		}
		is_valid
	}
}
