use std::{
	io::Write,
	path::PathBuf,
	time::Duration,
};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use itertools::Itertools;
use stopwatch::Stopwatch;
use szu::flush_print;
use ffmpeg_next::{
	software::{
		scaling::Context as Scaler,
		resampling::Context as Resampler,
	},
	util::{
		format::Sample as SampleFormat,
		frame::{
			video::Video as VideoFrame,
			audio::Audio as AudioFrame,
		}
	},
	codec::Context as CodecCtx,
	media::Type as MediaType,
};

use crate::{math::{Frac, Rect, Size}, AppError, LOG};
use crate::video::{
	self,
	cmd::packet::{self, Descriptor as VideoStreamDesc},
	oc_color::{formatters::*, RGB8, PaletteOr},
	image::Image,
};
use crate::audio::{
	packet::Descriptor as AudioStreamDesc,
	Config as AudioConfig,
};
use media_container::{SizedString, DescriptorHeader, MediaFile};

pub mod media_container;

pub struct EncoderArgs {
	pub in_path: PathBuf,
	pub out_path: PathBuf,

	pub begin: Option<Duration>,
	pub end: Option<Duration>,

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
		ictx: &'a ffmpeg_next::format::context::Input,
		stream_type: ffmpeg_next::media::Type
	) -> Option<(ffmpeg_next::Stream<'a>, ffmpeg_next::decoder::decoder::Decoder)> {
		let stream = ictx
			.streams()
			.best(stream_type)?;
		let codec_ctx = CodecCtx::from_parameters(stream.parameters())
			.expect("failed to create codec context");
		Some((stream, codec_ctx.decoder()))
	}

	//ffmpeg madness
	let mut ictx = ffmpeg_next::format::input(&args.in_path).expect("failed to create decoder");
	let (video_stream, mut video_decoder) = get_decoder(&ictx, MediaType::Video)
		.map(|(stream, decoder)| (stream, decoder.video().expect("failed to create video decoder")))
		.expect("we don't support audio only right now");
	let (audio_stream, mut audio_decoder) = get_decoder(&ictx, MediaType::Audio)
		.map(|(stream, decoder)| (stream, decoder.audio().expect("failed to create audio decoder")))
		.expect("we don't support video only right now");

	//compute in/out frame rates
	let in_frame_rate = video_decoder.frame_rate().map_or(0, |rate| szu::int_div_round!(rate.numerator(), rate.denominator()) as u16);

	let v_tb = Frac::from(video_stream.time_base()).cast::<i64>();
	let a_tb = Frac::from(audio_stream.time_base()).cast::<i64>();
	let v_end_ts = video_stream.start_time() + video_stream.duration();
	let a_end_ts = audio_stream.start_time() + audio_stream.duration();
	
	let ms_to_ts = |ms: u128, tb: Frac<i64>| (Frac::new(ms, 1000) / tb.try_cast::<u128>().unwrap()).into_int() as i64;
	let v_range_ts = args.begin.map_or(0, |begin| ms_to_ts(begin.as_millis(), v_tb))..=args.end.map_or(v_end_ts, |end| ms_to_ts(end.as_millis(), v_tb));
	let a_range_ts = args.begin.map_or(0, |begin| ms_to_ts(begin.as_millis(), a_tb))..=args.end.map_or(a_end_ts, |end| ms_to_ts(end.as_millis(), a_tb));

	//setup progress bar
	let multi_progress = MultiProgress::new();
	let progress_style = ProgressStyle::with_template("{msg} [{bar}] {pos}/{len} {eta}")
			.unwrap()
			.progress_chars("█▉▊▋▌▍▎▏ ");
	let video_frames = (Frac::new(video_stream.frames(), video_stream.duration()) * v_range_ts.try_len().unwrap() as i64).into_int();
	let video_progress: ProgressBar = multi_progress.add(ProgressBar::new(video_frames as u64)
		.with_style(progress_style.clone()))
		.with_message("video");
	let audio_frames = (Frac::new(audio_stream.frames(), audio_stream.duration()) * a_range_ts.try_len().unwrap() as i64).into_int();
	let audio_progress: ProgressBar = multi_progress.add(ProgressBar::new(audio_frames as u64)
		.with_style(progress_style.clone()))
		.with_message("audio");
	if !LOG {
		video_progress.tick();
		audio_progress.tick();
	}
	
	//setup down-scaler
	let content_size = Size::<usize>::new(video_decoder.width() as usize, video_decoder.height() as usize);
	let fit_size = args.streams_config.container_size.contain(content_size);

	let mut scaler = Scaler::get(
		video_decoder.format(),
		content_size.x as u32,
		content_size.y as u32,
		ffmpeg_next::util::format::Pixel::RGB24,
		fit_size.x as u32,
		fit_size.y as u32,
		ffmpeg_next::software::scaling::Flags::BILINEAR,
	).unwrap();

	// Setup resampler to f32 planar @ cli.rate, mono
	let in_fmt = audio_decoder.format();
	let in_ch = audio_decoder.channel_layout();
	let in_rate = audio_decoder.rate();

	let target_rate = 22050; // Target analysis sample rate (Hz)
	let target_layout = ffmpeg_next::channel_layout::ChannelLayout::MONO;
	let target_fmt = SampleFormat::F32(ffmpeg_next::format::sample::Type::Planar); // f32

	let mut resampler = Resampler::get(
		in_fmt, in_ch, in_rate,
		target_fmt, target_layout, target_rate,
	).unwrap();

	//setup SZT stream encoders
	let mut video_streams = args.streams_config.stream_descs_data
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
	
	let mut stream_frame_timers = video_streams.iter().map(|_| 0usize).collect_vec();
	let mut frame_out = VideoFrame::empty();
	let formatter = video::oc_color::formatters::HybridFormatter::new();
	let mut v_receive_and_process = |frame: &VideoFrame| {
		let p = frame.pts().unwrap_or(0);
		if p >= *v_range_ts.end() { return true; }
		
		let emit_streams = video_streams
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

		if LOG {
			println!();
			println!("emit: {}", if !emit_streams.is_empty() { "yes" } else { "no" });
		}

		if !emit_streams.is_empty() {
			let frame = stage("Frame  | Preamble  | Scale", || {
				scaler.run(&frame, &mut frame_out).unwrap();
				&frame_out
			});
			
			let img = stage("Frame  | Preamble  | Into Image", || Image::from(frame));
			let img = stage("Frame  | Preamble  | Resize", || img.resize(args.streams_config.container_size, args.fill_color));
			
			let emit_streams_len = emit_streams.len();
			let frame_progress = multi_progress.add(ProgressBar::new(emit_streams_len as u64)
				.with_style(progress_style.clone())
				.with_message("frame"));
			if !LOG && emit_streams_len > 1 {
				frame_progress.tick();
			}

			for (stream, emit_count) in emit_streams {
				let img = match &stream.source {
					Some(source) => stage("Stream | Preamble  | Crop", || img.crop(source)),
					None => img.clone(),
				};

				let img = stage("Stream | Process   | Deflate", || img.map(|p| formatter.deflate(PaletteOr::NonPalette(*p))));
				let img = stage("Stream | Process   | Inflate", || img.map(|p| formatter.inflate(*p)));
				let img = stage("Stream | Process   | Braille", || video::braille::as_braille(&img));
				let img = stage("Stream | Process   | B_Deflate ", || img.map(|braille| braille.map(|p| formatter.deflate(PaletteOr::NonPalette(*p)))));
				for _ in 0..emit_count {
					stage("Stream | Postamble | Cmd Gen", || stream.encoder.push_frame_braille(&img));
					//println!("{}", cmd::code_gen(&img.map(|braille| braille.into()), None, &formatter));
				}

				if !LOG && emit_streams_len > 1 {
					frame_progress.inc(1);
				}
			}
			if !LOG && emit_streams_len > 1 {
				frame_progress.finish_and_clear();
			}
		}
		if !LOG {
			video_progress.inc(1);
		}
		return false;
	};
	
	let mut pcm = Vec::new();
	let mut a_receive_and_process = |decoded: &AudioFrame| {
		let p = decoded.pts().unwrap_or(0);
		if p >= *a_range_ts.end() { return true; }
		
		let mut out = AudioFrame::empty();
		resampler.run(&decoded, &mut out).unwrap();

		let nb_samples = out.samples() as usize;
		let data = out.data(0);
		let slice: &[f32] = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const f32, nb_samples) }; //SAFETY: interpret as f32 slice
		pcm.extend_from_slice(slice);

		if !LOG {
			audio_progress.inc(1);
		}
		return false;
	};

	let video_stream_index = video_stream.index();
	let audio_stream_index = audio_stream.index();
	let mut video_done = false;
	let mut audio_done = false;
	let mut v_frame = VideoFrame::empty();
	let mut a_frame = AudioFrame::empty();
	for (stream, packet) in ictx.packets() {
		match stream.index() {
			index if index == video_stream_index => {
				video_decoder.send_packet(&packet).unwrap();
				while video_decoder.receive_frame(&mut v_frame).is_ok() {
					if v_receive_and_process(&v_frame) {
						video_done = true;
					}
				}
			},
			index if index == audio_stream_index => {
				audio_decoder.send_packet(&packet).unwrap();
				while audio_decoder.receive_frame(&mut a_frame).is_ok() {
					if a_receive_and_process(&a_frame) {
						audio_done = true;
					}
				}
			},
			_ => { },
		}
		if video_done && audio_done { break; }
	}
	video_decoder.send_eof().unwrap();
	while video_decoder.receive_frame(&mut v_frame).is_ok() {
		v_receive_and_process(&v_frame);
	}
	audio_decoder.send_eof().unwrap();
	while audio_decoder.receive_frame(&mut a_frame).is_ok() {
		a_receive_and_process(&a_frame);
	}

	let mut media_file = MediaFile::new();
	for stream in video_streams {
		stream.encoder.attach(&mut media_file);
	}

	// let audio_cfg = AudioConfig::default();
	// let samples = crate::audio::encode(&audio_cfg, &pcm);
	// let audio_encoder = crate::audio::packet::AudioEncoder::new(AudioStreamDesc {
	// 	header: DescriptorHeader { num_packets: 0, name: SizedString::new("main").unwrap() },
	// 	num_voices: audio_cfg.num_voices as u8,
	// });
	// audio_encoder.attach(&mut media_file);

	if LOG {
		println!();
	}
	stage("Postamble  | Writing...  ", || std::fs::write(&args.out_path, media_file.finalize())
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
