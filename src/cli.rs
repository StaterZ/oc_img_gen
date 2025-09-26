use std::{path::{Path, PathBuf}, time::Duration};
use clap::Parser;
use num_traits::ConstZero;

use crate::{
	encoder::{
		EncoderArgs,
		StreamsConfig,
		VideoStreamDescData,
		media_container::SizedString,
	},
	math::{Point, Rect, Size},
	video::{self, oc_color::RGB8},
	EXT,
};

pub fn parse_args() -> EncoderArgs {
	let args = Args::parse();
	validate_args(&args);
	
	let streams_config = compute_stream_config(&args);

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

	EncoderArgs {
		in_path: args.in_path,
		out_path,
		begin_time: args.begin_time,
		end_time: args.end_time,
		streams_config,
		fill_color: RGB8::new(0x000000),
	}
}

fn validate_args(args: &Args) {
	let mut is_bad = false;

	match args.mode {
		StreamMode::Single => {
			if args.stream_size.is_none() {
				eprintln!("--stream-size is required when --mode is 'Single' or 'Matrix'");
				is_bad = true;
			}
			if args.matrix_size.is_some() {
				eprintln!("--matrix-size is only valid when --mode is 'Matrix'");
				is_bad = true;
			}
			if args.streams_config.is_some() {
				eprintln!("--streams-config is only valid when --mode is 'Custom'");
				is_bad = true;
			}
		}
		StreamMode::Matrix => {
			if args.stream_size.is_none() {
				eprintln!("--stream-size is required when --mode is 'Single' or 'Matrix'");
				is_bad = true;
			}
			if args.matrix_size.is_none() {
				eprintln!("--matrix-size is required when --mode is 'Matrix'");
				is_bad = true;
			}
			if args.streams_config.is_some() {
				eprintln!("--streams-config is only valid when --mode is 'Custom'");
				is_bad = true;
			}
		}
		StreamMode::Custom => {
			if args.stream_size.is_some() {
				eprintln!("--stream-size is only valid when --mode is 'Single' or 'Matrix'");
				is_bad = true;
			}
			if args.matrix_size.is_some() {
				eprintln!("--matrix-size is only valid when --mode is 'Matrix'");
				is_bad = true;
			}
			if args.streams_config.is_none() {
				eprintln!("--streams-config is required when --mode is 'Custom'");
				is_bad = true;
			}
		}
	}

	if is_bad {
		std::process::exit(1);
	}
}

pub fn compute_stream_config(args: &Args) -> StreamsConfig {
	match args.mode {
		StreamMode::Single => create_main_stream(
			args.frame_rate,
			args.stream_size.unwrap(),
		),
		StreamMode::Matrix => create_matrix_streams(
			args.frame_rate,
			args.stream_size.unwrap(),
			args.matrix_size.unwrap(),
			args.matrix_gap_size
				.or_else(|| args.matrix_screen_size
					.map(|matrix_screen_size| {
						let gap = compute_gap_size(args.stream_size.unwrap().cast(), matrix_screen_size);
						println!("auto-gap: {}", gap);
						gap
					}))
				.unwrap_or(Size::ZERO),
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

fn create_main_stream(frame_rate: Option<u16>, stream_size: Size<u8>) -> StreamsConfig {
	let stream_descs_data = vec![VideoStreamDescData {
		name: SizedString::new("main").expect("stream name too long"),
		frame_rate,
		size: stream_size,
		source: None,
	}];

	StreamsConfig {
		stream_descs_data,
		container_size: stream_size.cast() * video::braille::SIZE,
	}
}

fn create_matrix_streams(
	frame_rate: Option<u16>,
	stream_size: Size<u8>,
	matrix_size: Size<usize>,
	matrix_gap_size: Size<usize>,
) -> StreamsConfig {
	let stream_input_size = stream_size.cast() * video::braille::SIZE;
	let container_size = matrix_size * stream_input_size + (matrix_size - 1) * matrix_gap_size;

	let stream_descs_data = (0..matrix_size.y)
		.flat_map(move |y| (0..matrix_size.x)
			.map(move |x| VideoStreamDescData {
				name: SizedString::new(&format!("{},{}", x, y)).expect("stream name too long"),
				frame_rate,
				size: stream_size,
				source: Some(Rect {
					pos: Point::new(x, y) * (stream_input_size + matrix_gap_size),
					size: stream_input_size,
				})
			}))
		.collect();

	StreamsConfig {
		stream_descs_data,
		container_size,
	}
}

fn create_streams_custom(_config_path: &Path) -> StreamsConfig {
	todo!("custom stream config");
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum StreamMode {
	Single,
	Matrix,
	Custom,
}

#[derive(Parser, Debug)]
#[clap(author = "StaterZ")]
pub struct Args {
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
	pub begin_time: Option<Duration>,

	#[arg(
		short = 'e',
		long = "end",
		help = "What frame to stop from (inclusive), stops at input end if omitted",
		value_parser = humantime::parse_duration,
	)]
	pub end_time: Option<Duration>,

	#[arg(
		short = 'f',
		long = "frame-rate",
		help = "The output framerate, copies input framerate if omitted",
	)]
	pub frame_rate: Option<u16>,
	

	#[arg(
		value_enum,
		short = 'm',
		long = "mode",
		help = "How to arrange and produce the streams",
		requires_if("Single", "stream_size"),
		requires_if("Matrix", "stream_size"),
		requires_if("Matrix", "matrix_size"),
		requires_if("Matrix", "matrix_screen_size"),
		requires_if("Custom", "streams_config"),
	)]
	pub mode: StreamMode,


	#[arg(
		long = "stream-size",
		help = "The size of the streams",
	)]
	pub stream_size: Option<Size<u8>>,
	
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
}
