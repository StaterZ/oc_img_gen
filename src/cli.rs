use std::{path::{Path, PathBuf}, time::Duration};
use clap::{ArgAction, Args, Parser};
use num_traits::{ConstOne, ConstZero};

use crate::{
	EXT, encoder::{
		AudioConfig, EncoderConfig, VideoConfig, VideoDescData
	}, math::{Frac, Point, Rect, Size}, video::{self, cmd::Machine, oc_color::RGB8}
};

pub fn parse_args() -> EncoderConfig {
	let args = Cli::parse();
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

	
	let video_config = args.video.as_ref().map(|video_args| {
		let ictx = ffmpeg_next::format::input(&args.in_path).expect("failed to create decoder");
		let stream = ictx.streams().best(ffmpeg_next::media::Type::Video).unwrap();
		let codec_ctx = ffmpeg_next::codec::Context::from_parameters(stream.parameters()).expect("failed to create codec context");
		let decoder = codec_ctx.decoder().video().unwrap();
		
		build_video_config(video_args, &decoder)
	});
	let audio_config = args.audio.as_ref().map(build_audio_config);
	
	EncoderConfig {
		in_path: args.in_path,
		out_path,
		range: args.begin..=args.end,
		video: video_config,
		audio: audio_config,
	}
}

fn build_video_config(args: &VideoOpts, stream: &ffmpeg_next::decoder::Video) -> VideoConfig {
	match args.mode.unwrap() { //SAFETY: unwrap safe due to argument validation in caller
		StreamMode::Single => {
			let stream_size = args.stream_size.unwrap_or_else(|| {
				let content_size = Size::<u32>::new(stream.width(), stream.height());
				let size = compute_largest_size_machine(content_size.cast::<usize>().ratio(), &Machine::T3);
				println!("auto-size: {} (largest possible gpu pixel count & width for ratio)", size);
				size
			}).try_cast().expect("stream size too large");

			create_main_stream(
				args.frame_rate,
				args.fill_color,
				args.cmds_per_sec,
				stream_size,
				args.filter,
			)
		},
		StreamMode::Matrix => create_matrix_streams(
			args.frame_rate,
			args.fill_color,
			args.cmds_per_sec,
			args.stream_size.unwrap_or_else(|| {
				let size = compute_largest_size_machine(Frac::ONE, &Machine::T3);
				println!("auto-size: {} (largest possible gpu pixel count & width for ratio)", size);
				size
			}).try_cast().expect("stream size too large"),
			args.matrix_size.unwrap(),
			args.matrix_gap_size
				.or_else(|| args.matrix_screen_size
					.map(|matrix_screen_size| {
						let gap = compute_gap_size(args.stream_size.unwrap(), matrix_screen_size);
						eprintln!("auto-gap: {}", gap);
						gap
					}))
				.unwrap_or(Size::ZERO),
			args.filter,
		),
		StreamMode::Custom => create_streams_custom(
			args.streams_config.as_ref().unwrap()
		),
	}
}

fn compute_gap_size(stream_size: Size<usize>, screen_size: Size<usize>) -> Size<usize> {
	const FIXED_POINT: usize = 2;
	const PIXEL_GAP: usize = 9; //it's '(2 + 0.25) * 2' but we use x2 fixed point to remove the floats
	const SUB_PIXEL_SIZE: Size<usize> = video::braille::SIZE;
	const MINECRAFT_PIXELS: usize = 16;
	//https://www.desmos.com/calculator/balbctweiy
	(stream_size * SUB_PIXEL_SIZE * PIXEL_GAP) / (screen_size * (MINECRAFT_PIXELS * FIXED_POINT) - PIXEL_GAP)
}

pub fn compute_largest_size_machine(ratio: Frac<usize>, machine: &Machine) -> Size<usize> {
	compute_largest_size(ratio, machine.max_screen_size.area(), machine.max_screen_size.x)
}

pub fn compute_largest_size(ratio: Frac<usize>, max_pixels: usize, max_width: usize) -> Size<usize> {
	debug_assert!(ratio.numerator > 0 && ratio.denominator > 0, "Invalid ratio");
	debug_assert!(max_pixels > 0, "max_pixels must be positive");

	// Start by assuming width is limited by pixel count
	// width * height <= max_pixels
	// height = width / ratio
	// so: width^2 / ratio <= max_pixels
	// => width <= sqrt(max_pixels * ratio)
	const SUB_PIXEL_SIZE: Size<usize> = video::braille::SIZE;
	let ratio = ratio / SUB_PIXEL_SIZE.ratio();
	let width_limit = (ratio * max_pixels).sqrt();

	let width = width_limit.min(max_width.into());
	let height = width / ratio;

	Size::new(width.into_int_trunc(), height.into_int_trunc())
}

fn create_main_stream(
	frame_rate: Option<Frac<u16>>,
	fill_color: RGB8,
	cmds_per_sec: Option<usize>,
	stream_size: Size<u8>,
	filter: Option<VideoFilter>,
) -> VideoConfig {
	let stream_descs_data = vec![VideoDescData {
		name: "main".to_string(),
		frame_rate,
		size: stream_size,
		source_area: None,
		filter,
	}];

	VideoConfig {
		stream_descs_data,
		container_size: stream_size.cast() * video::braille::SIZE,
		fill_color,
		cmds_per_sec,
	}
}

fn create_matrix_streams(
	frame_rate: Option<Frac<u16>>,
	fill_color: RGB8,
	cmds_per_sec: Option<usize>,
	stream_size: Size<u8>,
	matrix_size: Size<usize>,
	matrix_gap_size: Size<usize>,
	filter: Option<VideoFilter>,
) -> VideoConfig {
	let stream_input_size = stream_size.cast() * video::braille::SIZE;
	let container_size = matrix_size * stream_input_size + (matrix_size - 1) * matrix_gap_size;

	let stream_descs_data = (0..matrix_size.y)
		.flat_map(move |y| (0..matrix_size.x)
			.map(move |x| VideoDescData {
				name: format!("{},{}", x, y),
				frame_rate,
				size: stream_size,
				source_area: Some(Rect {
					pos: Point::new(x, y) * (stream_input_size + matrix_gap_size),
					size: stream_input_size,
				}),
				filter
			}))
		.collect();

	VideoConfig {
		stream_descs_data,
		container_size,
		fill_color,
		cmds_per_sec,
	}
}

fn create_streams_custom(_config_path: &Path) -> VideoConfig {
	todo!("custom stream config");
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum StreamMode {
	Single,
	Matrix,
	Custom,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum VideoFilter {
	Monochrome,
	Grayscale,
}

pub fn build_audio_config(args: &AudioOpts) -> AudioConfig {
	AudioConfig {
		name: "main".to_string(),
		analysis_rate: args.analysis_rate,
		fft_window_size: args.fft_window_size,
		hop_length: args.hop_length.unwrap_or(args.fft_window_size / 2),
		normalize: args.normalize,
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
	)]
	pub in_path: PathBuf,

	#[arg(
		short = 'o',
		long = "out",
		help = "Path to where to the output SZT file, saves at in_path with .szt extension if omitted",
	)]
	pub out_path: Option<PathBuf>,


	#[arg(
		short = 'b',
		long = "begin",
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
		requires_if("Single", "stream_size"),
		requires_if("Single", "filter"),
		requires_if("Matrix", "stream_size"),
		requires_if("Matrix", "filter"),
		requires_if("Matrix", "matrix_size"),
		requires_if("Matrix", "matrix_screen_size"),
		requires_if("Custom", "streams_config"),
	)]
	pub mode: Option<StreamMode>,

	#[arg(
		short = 'f',
		long = "frame-rate",
		help = "The output framerate, copies input framerate if omitted",
	)]
	pub frame_rate: Option<Frac<u16>>,
	
	#[arg(
		long = "fill-color",
		help = "color of the padding around the video",
		default_value_t = RGB8::new(0x000000),
	)]
	pub fill_color: RGB8,

	#[arg(
		long = "cps",
		help = "how many commands to allow per second",
	)]
	pub cmds_per_sec: Option<usize>,

	#[arg(
		long = "stream-size",
		help = "The size of the streams",
	)]
	pub stream_size: Option<Size<usize>>,
	
	#[arg(
		long = "matrix-size",
		help = "How many grid cells (streams) to create",
	)]
	pub matrix_size: Option<Size<usize>>,

	#[arg(
		long = "matrix-gap-size",
		help = "pixels to skip between matrix cells, sets to 0 to omitted",
	)]
	pub matrix_gap_size: Option<Size<usize>>,
	
	#[arg(
		long = "matrix-screen-size",
		help = "screen size of matrix segments, this is used to derive the matrix gap size",
	)]
	pub matrix_screen_size: Option<Size<usize>>,
	
	#[arg(
		long = "streams-config",
		help = "path to json streams config file",
	)]
	pub streams_config: Option<PathBuf>,

	#[arg(
		long = "filter",
		help = "what to pass the pixels through before encoding",
	)]
	pub filter: Option<VideoFilter>,
}

impl VideoOpts {
	fn validate(&self) -> bool {
		let mut is_valid = true;
		match self.mode {
			None => {
				eprintln!("--mode is required");
				is_valid = false;
			}
			Some(mode) => match mode {
				StreamMode::Single => {
					if self.matrix_size.is_some() {
						eprintln!("--matrix-size is only valid when --mode is 'Matrix'");
						is_valid = false;
					}
					if self.streams_config.is_some() {
						eprintln!("--streams-config is only valid when --mode is 'Custom'");
						is_valid = false;
					}
				}
				StreamMode::Matrix => {
					if self.matrix_size.is_none() {
						eprintln!("--matrix-size is required when --mode is 'Matrix'");
						is_valid = false;
					}
					if self.streams_config.is_some() {
						eprintln!("--streams-config is only valid when --mode is 'Custom'");
						is_valid = false;
					}
				}
				StreamMode::Custom => {
					if self.stream_size.is_some() {
						eprintln!("--stream-size is only valid when --mode is 'Single' or 'Matrix'");
						is_valid = false;
					}
					if self.matrix_size.is_some() {
						eprintln!("--matrix-size is only valid when --mode is 'Matrix'");
						is_valid = false;
					}
					if self.streams_config.is_none() {
						eprintln!("--streams-config is required when --mode is 'Custom'");
						is_valid = false;
					}
					if self.filter.is_some() {
						eprintln!("--filter is only valid when --mode is 'Single' or 'Matrix'");
						is_valid = false;
					}
				}
			}
		}
		is_valid
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
		long = "voices",
		default_value_t = 8,
		help = "How many voices to use (8 channels per sound card)",
	)]
	pub num_voices: usize,

	#[arg(
		short = 'n',
		long = "normalize",
		default_value_t = true,
		action = ArgAction::SetTrue,
		help = "Normalize output overall loudness",
	)]
	pub normalize: bool,
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
