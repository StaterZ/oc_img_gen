use std::{
	io::Write,
	path::PathBuf,
	time::Duration,
};
use deku::DekuContainerWrite;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use itertools::Itertools;
use stopwatch::Stopwatch;
use szu::flush_print;

use crate::{math::{Rect, Size}, AppError, LOG};
use crate::video::{
	self,
	cmd::packet::{self, Descriptor as VideoStreamDesc},
	oc_color::{formatters::*, RGB8, PaletteOr},
	image::Image,
};
use media_container::{SizedString, DescriptorHeader, MediaFile};

pub mod media_container;

pub struct EncoderArgs {
	pub in_path: PathBuf,
	pub out_path: PathBuf,

	pub begin_time: Option<Duration>,
	pub end_time: Option<Duration>,

	pub streams_config: StreamsConfig,
	pub fill_color: RGB8,
}

pub struct StreamsConfig {
	pub stream_descs_data: Vec<VideoStreamDescData>,
	pub container_size: Size<usize>,
}

pub struct VideoStreamDescData {
	pub name: SizedString<u8>,
	pub frame_rate: Option<u16>,
	pub size: Size<u8>,
	pub source: Option<Rect<usize>>,
}

struct VideoEncoderData {
	encoder: packet::VideoEncoder,
	source: Option<Rect<usize>>,
}

pub fn encode(args: EncoderArgs) -> anyhow::Result<()> {
	fn get_decoder<'a>(
		input_format_context: &'a ffmpeg_next::format::context::Input,
		stream_type: ffmpeg_next::media::Type
	) -> Option<(ffmpeg_next::Stream<'a>, ffmpeg_next::decoder::decoder::Decoder)> {
		let stream = input_format_context
			.streams()
			.best(stream_type)?;
		let codec_context = ffmpeg_next::codec::Context::from_parameters(stream.parameters())
			.expect("failed to create codec context");
		Some((stream, codec_context.decoder()))
	}

	//ffmpeg madness
	let mut input_format_context = ffmpeg_next::format::input(&args.in_path).expect("failed to create decoder");
	let (video_stream, mut video_decoder) = get_decoder(&input_format_context, ffmpeg_next::media::Type::Video)
		.map(|(stream, decoder)| (stream, decoder.video().expect("failed to create video decoder")))
		.expect("we don't support audio only right now");
	let audio = get_decoder(&input_format_context, ffmpeg_next::media::Type::Audio)
		.map(|(stream, decoder)| (stream, decoder.audio().expect("failed to create audio decoder")));

	//compute in/out frame rates
	let in_frame_rate = video_decoder.frame_rate().map_or(0, |rate| szu::int_div_round!(rate.numerator(), rate.denominator()) as u16);
	
	let begin_frame = args.begin_time.map(|begin_time| (begin_time.as_millis() * in_frame_rate as u128 / 1000) as usize).unwrap_or(0);
	let num_frames = std::cmp::max(video_stream.frames() as usize, 1);
	let last_frame = args.end_time.map(|end_time| (end_time.as_millis() * in_frame_rate as u128 / 1000) as usize).unwrap_or(num_frames - 1);

	//setup progress bar
	let multi_progress = MultiProgress::new();

	let num_frames_to_process = (last_frame + 1) - begin_frame;
	let frames_progress = multi_progress.add(ProgressBar::new(num_frames_to_process as u64)
		.with_style(ProgressStyle::with_template("[{bar}] {pos}/{len} {eta}")
			.unwrap()
			.progress_chars("█▉▊▋▌▍▎▏ ")));
	if !LOG {
		frames_progress.tick();
	}

	//setup down-scaler
	let content_size = Size::<usize>::new(video_decoder.width() as usize, video_decoder.height() as usize);
	let fit_size = args.streams_config.container_size.contain(content_size);

	let mut scaler = ffmpeg_next::software::scaling::Context::get(
		video_decoder.format(),
		content_size.x as u32,
		content_size.y as u32,
		ffmpeg_next::util::format::Pixel::RGB24,
		fit_size.x as u32,
		fit_size.y as u32,
		ffmpeg_next::software::scaling::Flags::BILINEAR,
	).unwrap();

	//setup SZT stream writers
	let mut streams = args.streams_config.stream_descs_data
		.into_iter()
		.map(|stream_data| VideoEncoderData {
			encoder: packet::VideoEncoder::new({
				let size = if stream_data.source.is_some() {
					stream_data.size
				} else {
					(fit_size / video::braille::SIZE).try_cast().unwrap()
				};
				VideoStreamDesc {
					header: DescriptorHeader {
						num_packets: 0,
						name: stream_data.name,
					},
					frame_rate: stream_data.frame_rate.unwrap_or(in_frame_rate),
					size,
				}
			}),
			source: stream_data.source,
		})
		.collect_vec();
	
	//loop frames
	let mut frame_index = 0;
	let mut stream_frame_timers = streams.iter().map(|_| 0usize).collect_vec();
	let mut receive_and_process_frames = |decoder: &mut ffmpeg_next::decoder::Video| {
		let mut decoded_frame = ffmpeg_next::frame::Video::empty();
		while decoder.receive_frame(&mut decoded_frame).is_ok() {
			if frame_index < begin_frame {
				frame_index += 1;
				continue;
			}
			if frame_index > last_frame {
				frame_index += 1;
				return false;
			}
			
			let emit_streams = streams
				.iter_mut()
				.zip_eq(stream_frame_timers.iter_mut())
				.filter_map(|(stream, time)| {
					let in_frame_rate = in_frame_rate as usize;
					*time += stream.encoder.get_desc().frame_rate as usize;
					if *time >= in_frame_rate {
						let emit_count = if in_frame_rate == 0 { 1usize } else { 
							let emit_count = *time / in_frame_rate;
							*time %= in_frame_rate;
							emit_count
						};
						return Some((stream, emit_count));
					}
					return None;
				}).collect_vec();

			if emit_streams.is_empty() {
				let frame = stage("Frame  | Preamble  | Scale", || {
					let mut frame = ffmpeg_next::frame::Video::empty();
					scaler.run(&decoded_frame, &mut frame).unwrap();
					frame
				});
				
				let img = stage("Frame  | Preamble  | Into Image", || Image::from(frame));
				let img = stage("Frame  | Preamble  | Resize", || img.resize(args.streams_config.container_size, args.fill_color));
				
				let frame_progress = multi_progress.add(ProgressBar::new(emit_streams.len() as u64)
					.with_style(ProgressStyle::with_template("[{bar}] {pos}/{len}")
						.unwrap()
						.progress_chars("█▉▊▋▌▍▎▏ ")));
				if !LOG {
					frame_progress.tick();
				}
				
				for (stream, emit_count) in emit_streams {
					if LOG {
						println!();
					}

					let img = match &stream.source {
						Some(source) => stage("Stream | Preamble  | Crop", || img.crop(source)),
						None => img.clone(),
					};

					let formatter = video::oc_color::formatters::HybridFormatter::new();
					let img = stage("Stream | Process   | Deflate", || img.map(|p| formatter.deflate(PaletteOr::NonPalette(*p))));
					let img = stage("Stream | Process   | Inflate", || img.map(|p| formatter.inflate(*p)));
					let img = stage("Stream | Process   | Braille", || video::braille::as_braille(&img));
					let img = stage("Stream | Process   | B_Deflate ", || img.map(|braille| braille.map(|p| formatter.deflate(PaletteOr::NonPalette(*p)))));
					for _ in 0..emit_count {
						stage("Stream | Postamble | Cmd Gen", || stream.encoder.push_frame_braille(&img));
						//println!("{}", cmd::code_gen(&img.map(|braille| braille.into()), None, &formatter));
					}

					if !LOG {
						frame_progress.inc(1);
					}
				}
				frame_progress.finish_and_clear();
			}
			frame_index += 1;
			if !LOG {
				frames_progress.inc(1);
			}
		}

		return true;
	};

	let receive_and_process_audio_frames = |_decoder: &mut ffmpeg_next::decoder::Audio| {
		println!("hello!");
		false
	};

	let video_stream_index = video_stream.index();
	let mut audio = audio.map(|(stream, decoder)| (stream.index(), decoder));
	let stream_count = if audio.is_some() { 2 } else { 1 };
	for (stream, packet) in input_format_context.packets() {
		let pts_sec = packet.pts().unwrap_or(0) * video_decoder.time_base();
		if pts_sec >  {

		}

		let mut done_streams = 0;
		match stream.index() {
			index if index == video_stream_index => {
				video_decoder.send_packet(&packet).unwrap();
				if !receive_and_process_frames(&mut video_decoder) {
					done_streams += 1;
				}
			},
			index if audio.as_ref().map_or(false, |(stream_index, _decoder)| index == *stream_index) => {
				let (_audio_stream_index, audio_decoder) = audio.as_mut().unwrap();
				audio_decoder.send_packet(&packet).unwrap();
				if !receive_and_process_audio_frames(audio_decoder) {
					done_streams += 1;
				}
			},
			_ => { },
		}
		if done_streams == stream_count { break; }
	}
	video_decoder.send_eof().unwrap();
	receive_and_process_frames(&mut video_decoder);
	if let Some((_audio_stream_index, audio_decoder)) = audio.as_mut() {
		audio_decoder.send_eof().unwrap();
		receive_and_process_audio_frames(audio_decoder);
	}

	frames_progress.finish();
	
	let mut media_file = MediaFile::new();
	for stream in streams {
		stream.encoder.attach(&mut media_file);
	}

	if LOG {
		println!();
	}
	stage("Postamble  | Writing...  ", || std::fs::write(&args.out_path, media_file.to_bytes().unwrap())
		.map_err(AppError::WriteFailed))?;

	println!("All Done! saved to: {}", args.out_path.display());
	Ok(())
}

fn stage<B>(title: &str, f: impl FnOnce() -> B) -> B {
	if !LOG {
		f()
	} else {
		flush_print!("{}", title);
		let mut timer = Stopwatch::start_new();
		let output = f();
		timer.stop();
		println!(" time: {}ms", timer.elapsed().as_millis());
		output
	}
}
