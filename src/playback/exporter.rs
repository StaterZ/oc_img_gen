use std::{path::{Path, PathBuf}, time::Duration};

use clap::Parser;
use deku::DekuContainerRead;
use ffmpeg_next as ffmpeg;
use itertools::Itertools;

use crate::{encoder::media_container::{MediaFile, PacketContent}, math::*, video::braille};
use super::{self as shared, DrawOptions, DrawTarget, Gpu, VoiceState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ExportFormat {
	Mp4,
	Mp3,
	Png,
}

#[derive(Parser, Debug)]
#[command(author, version)]
pub struct Cli {
	#[arg(short = 'i', long = "in", help = "Path to the file to export")]
	pub in_path: PathBuf,

	#[arg(short = 'o', long = "out", help = "output path, defaults to the input file stem with the picked extension")]
	pub out_path: Option<PathBuf>,

	#[arg(short = 'f', long = "format", help = "force an output format instead of picking one from the streams")]
	pub format: Option<ExportFormat>,

	#[arg(short = 'd', long = "diff", help = "visualize difference from the last frame")]
	pub diff: bool,

	#[arg(short = 'r', long = "raw", help = "visualize the underlying braille symbols")]
	pub show_raw: bool,

	#[arg(long = "vst", help = "seconds of smoothing to apply to each voice, reduces popping", default_value_t = humantime::Duration::from(std::time::Duration::from_millis(1)))]
	pub voice_smooth_time: humantime::Duration,

	#[arg(long = "sample-rate", help = "audio sample rate for mp4 and mp3 export", default_value_t = 44_100)]
	pub sample_rate: u32,

	#[arg(long = "video-bitrate", help = "target video bitrate in bits per second", default_value_t = 4_000_000)]
	pub video_bitrate: usize,

	#[arg(long = "audio-bitrate", help = "target audio bitrate in bits per second", default_value_t = 192_000)]
	pub audio_bitrate: usize,
}

fn pick_format(file: &MediaFile) -> anyhow::Result<ExportFormat> {
	let has_video = file.stream_descs.iter().any(|desc| desc.content.is_video());
	let has_audio = file.stream_descs.iter().any(|desc| desc.content.is_audio());
	let is_one_frame = file.stream_descs
		.iter()
		.any(|desc| desc.content.is_video() && desc.num_packets == 1);

	Ok(match (has_video, has_audio) {
		(true, true) if is_one_frame => ExportFormat::Mp3, //music with cover
		(true, false) if is_one_frame => ExportFormat::Png, //image
		(true, _) => ExportFormat::Mp4, //video with or without audio
		(false, true) => ExportFormat::Mp3, //pure music
		(false, false) => anyhow::bail!("The input file has no video or audio streams to export."),
	})
}

fn default_out_path(in_path: &Path, format: ExportFormat) -> PathBuf {
	let stem = shared::stem(in_path).unwrap_or("out");
	let ext = match format {
		ExportFormat::Mp4 => "mp4",
		ExportFormat::Mp3 => "mp3",
		ExportFormat::Png => "png",
	};
	PathBuf::from(format!("{stem}.{ext}"))
}

pub fn export(args: Cli) -> anyhow::Result<()> {
	let raw = std::fs::read(&args.in_path)?;
	let file = MediaFile::from_bytes((raw.as_slice(), 0))?.1;
	anyhow::ensure!(file.header.num_streams > 0, "The input file has no streams.");

	let format = match args.format {
		Some(format) => format,
		None => pick_format(&file)?,
	};
	let out_path = args.out_path.clone().unwrap_or_else(|| default_out_path(&args.in_path, format));

	ffmpeg_next::init()?;
	ffmpeg_next::util::log::set_level(ffmpeg_next::log::Level::Quiet);
	//ffmpeg_tracing::install();
	match format {
		ExportFormat::Png => export_png(&file, &out_path, &args),
		ExportFormat::Mp4 => export_mp4(&file, &out_path, &args),
		ExportFormat::Mp3 => export_mp3(&file, &out_path, &args),
	}
}

fn export_png(file: &MediaFile, out_path: &Path, args: &Cli) -> anyhow::Result<()> {
	let (stream_index, desc) = file.stream_descs
		.iter()
		.enumerate()
		.find(|(_, desc)| desc.content.is_video())
		.ok_or_else(|| anyhow::anyhow!("The input file has no video stream."))?;

	let video_desc = desc.content.as_video().unwrap();
	let size_cells = video_desc.size.cast::<usize>();
	let size_pixels = size_cells * braille::SIZE;

	let mut gpu = Gpu::new();
	let opts = DrawOptions { diff: args.diff, show_raw: args.show_raw };
	let mut target = DrawTarget::new(size_pixels, size_cells, 0xff00ff);

	let frame = file.packets
		.iter()
		.filter(|packet| packet.stream_id as usize == stream_index)
		.find_map(|packet| match &packet.content {
			PacketContent::Video(frame) => Some(frame),
			_ => None,
		})
		.ok_or_else(|| anyhow::anyhow!("The video stream has no frames."))?;

	target.draw_packet(&mut gpu, &opts, frame)?;

	shared::write_image(out_path, target.image
		.iter()
		.map(|p| palette::Srgb::<u8>::from_u32::<palette::rgb::channels::Argb>(*p)))?;

	eprintln!("Wrote {}", out_path.display());
	Ok(())
}

/// A packed ARGB pixel from `DrawTarget::image` split into R, G, B bytes.
fn argb_to_rgb_bytes(pixel: u32) -> [u8; 3] {
	let color = palette::Srgb::<u8>::from_u32::<palette::rgb::channels::Argb>(pixel);
	[color.red, color.green, color.blue]
}

struct VideoTrack {
	stream_id: u8,
	size: Size<u32>,
	target: DrawTarget,
	gpu: Gpu,
	stream_time_base: Frac<i32>,
	ost_index: usize,
	encoder: ffmpeg::encoder::Video,
	scaler: ffmpeg::software::scaling::Context,
}

fn open_video_track(
	octx: &mut ffmpeg::format::context::Output,
	desc: &crate::encoder::media_container::Descriptor<crate::encoder::media_container::DescriptorContent>,
	stream_id: u8,
	args: &Cli,
) -> anyhow::Result<VideoTrack> {
	let video_desc = desc.content.as_video().unwrap();
	let size_cells = video_desc.size.cast::<usize>();
	let size_pixels = size_cells * braille::SIZE;

	// h265 needs even width and height.
	let size = Size::new(
		(size_pixels.w as u32 + 1) & !1,
		(size_pixels.h as u32 + 1) & !1,
	);

	let rate = desc.rate.cast::<i32>();

	let codec = ffmpeg::encoder::find(ffmpeg::codec::Id::H265)
		.ok_or_else(|| anyhow::anyhow!("No h265 encoder is available. Build ffmpeg with libx265."))?;

	let mut encoder_ctx = ffmpeg::codec::context::Context::new_with_codec(codec)
		.encoder()
		.video()?;
	encoder_ctx.set_width(size.w);
	encoder_ctx.set_height(size.h);
	encoder_ctx.set_format(ffmpeg::format::Pixel::YUV420P);
	encoder_ctx.set_time_base(rate);
	encoder_ctx.set_bit_rate(args.video_bitrate);
	if octx.format().flags().contains(ffmpeg::format::Flags::GLOBAL_HEADER) {
		encoder_ctx.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
	}

	let mut ost = octx.add_stream(codec)?;
	let ost_index = ost.index();

	let encoder = encoder_ctx.open_as(codec)?;
	ost.set_parameters(&encoder);
	ost.set_time_base(rate);

	let scaler = ffmpeg::software::scaling::Context::get(
		ffmpeg::format::Pixel::RGB24,
		size.w,
		size.h,
		ffmpeg::format::Pixel::YUV420P,
		size.w,
		size.h,
		ffmpeg::software::scaling::Flags::BILINEAR,
	)?;

	Ok(VideoTrack {
		stream_id,
		size,
		stream_time_base: rate,
		target: DrawTarget::new(size_pixels, size_cells, 0xff00ff),
		gpu: Gpu::new(),
		ost_index,
		encoder,
		scaler,
	})
}

impl VideoTrack {
	fn push_frame(&mut self, opts: &DrawOptions, frame: &crate::video::cmd::packet::Frame) -> anyhow::Result<()> {
		self.target.draw_packet(&mut self.gpu, opts, frame)?;

		let mut rgb_frame = ffmpeg::util::frame::Video::new(ffmpeg::format::Pixel::RGB24, self.size.w, self.size.h);
		let mut yuv_frame = ffmpeg::util::frame::Video::new(ffmpeg::format::Pixel::YUV420P, self.size.w, self.size.h);

		let size = self.size.cast::<usize>();
		let stride = rgb_frame.stride(0);
		let data = rgb_frame.data_mut(0);

		for (y, row) in self.target.image
			.buffer()
			.iter()
			.chunks(self.target.image.size().w)
			.into_iter()
			.take(size.h)
			.enumerate()
		{
			for (x, pixel) in row.take(size.w).enumerate() {
				let [r, g, b] = argb_to_rgb_bytes(*pixel);
				let offset = y * stride + x * 3;
				data[offset] = r;
				data[offset + 1] = g;
				data[offset + 2] = b;
			}
		}

		self.scaler.run(&rgb_frame, &mut yuv_frame)?;

		let pts = (self.target.frame_index - 1) as i64;
		yuv_frame.set_pts(Some(pts));

		self.encoder.send_frame(&yuv_frame)?;
		Ok(())
	}

	fn drain_packets(&mut self, octx: &mut ffmpeg::format::context::Output) -> anyhow::Result<()> {
		let stream_time_base = octx.stream(self.ost_index).unwrap().time_base();

		let mut packet = ffmpeg::Packet::empty();
		while self.encoder.receive_packet(&mut packet).is_ok() {
			packet.set_stream(self.ost_index);
			packet.rescale_ts(self.stream_time_base, stream_time_base);
			packet.write_interleaved(octx)?;
		}
		Ok(())
	}

	fn finish(&mut self, octx: &mut ffmpeg::format::context::Output) -> anyhow::Result<()> {
		self.encoder.send_eof()?;
		self.drain_packets(octx)
	}
}

struct AudioTrack {
	stream_id: u8,
	voices: Vec<VoiceState>,
	targets: Vec<crate::audio::VoiceStateFlt>,
	smooth_coefficient: f32,
	sample_rate: f32,
	next_frame_index: u64,
	samples_written: i64,
	ost_index: usize,
	encoder: ffmpeg::encoder::Audio,
	resampler: ffmpeg::software::resampling::Context,
	frame: ffmpeg::util::frame::Audio,
	frame_fill: usize,
}

fn open_audio_track(
	octx: &mut ffmpeg::format::context::Output,
	desc: &crate::encoder::media_container::Descriptor<crate::encoder::media_container::DescriptorContent>,
	stream_id: u8,
	args: &Cli,
	codec_id: ffmpeg::codec::Id,
) -> anyhow::Result<AudioTrack> {
	let audio_desc = desc.content.as_audio().unwrap();
	let num_voices = audio_desc.num_voices as usize;

	let codec = ffmpeg::encoder::find(codec_id)
		.ok_or_else(|| anyhow::anyhow!("No encoder is available for {:?}.", codec_id))?;

	let sample_rate = args.sample_rate;
	let channels = ffmpeg::channel_layout::ChannelLayout::MONO;

	let mut encoder_ctx = ffmpeg::codec::context::Context::new_with_codec(codec)
		.encoder()
		.audio()?;
	encoder_ctx.set_rate(sample_rate as i32);
	encoder_ctx.set_channel_layout(channels);
	encoder_ctx.set_format(encoder_ctx.codec().unwrap().audio()?.formats().unwrap().next()
		.ok_or_else(|| anyhow::anyhow!("The audio encoder has no supported sample formats."))?);
	encoder_ctx.set_bit_rate(args.audio_bitrate);
	let time_base = ffmpeg::Rational(1, sample_rate as i32);
	encoder_ctx.set_time_base(time_base);
	if octx.format().flags().contains(ffmpeg::format::Flags::GLOBAL_HEADER) {
		encoder_ctx.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
	}

	let mut ost = octx.add_stream(codec)?;
	let ost_index = ost.index();

	let encoder = encoder_ctx.open_as(codec)?;
	ost.set_parameters(&encoder);
	ost.set_time_base(time_base);

	let resampler = ffmpeg::software::resampling::Context::get(
		ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
		channels,
		sample_rate,
		encoder.format(),
		channels,
		sample_rate,
	)?;

	let frame_samples = if encoder.frame_size() > 0 { encoder.frame_size() as usize } else { 1024 };
	let mut frame = ffmpeg::util::frame::Audio::new(
		ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
		frame_samples,
		channels,
	);
	frame.set_rate(sample_rate);

	Ok(AudioTrack {
		stream_id,
		voices: vec![VoiceState::new(0.0); num_voices],
		targets: vec![crate::audio::VoiceStateFlt::default(); num_voices],
		smooth_coefficient: shared::smooth_coefficient(sample_rate as f32, args.voice_smooth_time.into()),
		sample_rate: sample_rate as f32,
		next_frame_index: 0,
		samples_written: 0,
		ost_index,
		encoder,
		resampler,
		frame,
		frame_fill: 0,
	})
}

impl AudioTrack {
	fn set_targets(&mut self, sample: &crate::audio::packet::Sample) {
		for (target, voice) in self.targets.iter_mut().zip_eq(&sample.voices) {
			*target = shared::voice_target_from_sample(voice);
		}
	}

	fn render_until(&mut self, end_secs: f64, octx: &mut ffmpeg::format::context::Output) -> anyhow::Result<()> {
		let target_samples = (end_secs * self.sample_rate as f64).round() as i64;

		while self.samples_written < target_samples {
			let mut sample = 0.0f32;
			for (voice, target) in self.voices.iter_mut().zip_eq(&self.targets) {
				sample += voice.advance(target, self.sample_rate, self.smooth_coefficient);
			}

			self.frame.plane_mut::<f32>(0)[self.frame_fill] = sample;
			self.frame_fill += 1;
			self.samples_written += 1;

			if self.frame_fill == self.frame.samples() {
				self.flush_frame(octx)?;
			}
		}
		Ok(())
	}

	fn flush_frame(&mut self, octx: &mut ffmpeg::format::context::Output) -> anyhow::Result<()> {
		if self.frame_fill == 0 { return Ok(()); }

		// Only `frame_fill` samples hold real data on the final, partial
		// frame; the rest of the buffer is stale audio from the previous
		// frame. Tell the resampler to only read the valid part.
		self.frame.set_samples(self.frame_fill);
		self.frame.set_pts(Some(self.samples_written - self.frame_fill as i64));

		let mut resampled = ffmpeg::util::frame::Audio::empty();
		self.resampler.run(&self.frame, &mut resampled)?;
		resampled.set_pts(self.frame.pts());

		self.encoder.send_frame(&resampled)?;
		self.drain_packets(octx)?;

		self.frame_fill = 0;
		Ok(())
	}

	fn drain_packets(&mut self, octx: &mut ffmpeg::format::context::Output) -> anyhow::Result<()> {
		let stream_time_base = octx.stream(self.ost_index).unwrap().time_base();

		let mut packet = ffmpeg::Packet::empty();
		while self.encoder.receive_packet(&mut packet).is_ok() {
			packet.set_stream(self.ost_index);
			packet.rescale_ts(ffmpeg::Rational(1, self.sample_rate as i32), stream_time_base);
			packet.write_interleaved(octx)?;
		}
		Ok(())
	}

	fn finish(&mut self, octx: &mut ffmpeg::format::context::Output) -> anyhow::Result<()> {
		self.flush_frame(octx)?;
		self.encoder.send_eof()?;
		self.drain_packets(octx)
	}
}

fn export_mp4(file: &MediaFile, out_path: &Path, args: &Cli) -> anyhow::Result<()> {
	let mut octx = ffmpeg::format::output(out_path)?;

	let mut video_tracks: Vec<VideoTrack> = file.stream_descs
		.iter()
		.enumerate()
		.filter(|(_, desc)| desc.content.is_video())
		.map(|(index, desc)| open_video_track(&mut octx, desc, index as u8, args))
		.try_collect()?;

	let mut audio_track = file.stream_descs
		.iter()
		.enumerate()
		.find(|(_, desc)| desc.content.is_audio())
		.map(|(index, desc)| open_audio_track(&mut octx, desc, index as u8, args, ffmpeg::codec::Id::AAC))
		.transpose()?;

	octx.write_header()?;

	let progress = indicatif::ProgressBar::new(file.packets.len() as u64)
		.with_style(shared::progress_style());
	let opts = DrawOptions { diff: args.diff, show_raw: args.show_raw };

	for packet in &file.packets {
		match &packet.content {
			PacketContent::Video(frame) => {
				if let Some(track) = video_tracks.iter_mut().find(|track| track.stream_id == packet.stream_id) {
					track.push_frame(&opts, frame)?;
					track.drain_packets(&mut octx)?;
				}
			},
			PacketContent::Audio(sample) => {
				if let Some(track) = audio_track.as_mut().filter(|track| track.stream_id == packet.stream_id) {
					track.set_targets(sample);
					track.next_frame_index += 1;

					let desc = &file.stream_descs[track.stream_id as usize];
					let ts_end = Duration::from(desc.rate.cast::<u32>() * track.next_frame_index as u32);
					track.render_until(ts_end.as_secs_f64(), &mut octx)?;
				}
			},
		}
		progress.inc(1);
	}

	for track in video_tracks.iter_mut() {
		track.finish(&mut octx)?;
	}
	if let Some(track) = audio_track.as_mut() {
		track.finish(&mut octx)?;
	}

	octx.write_trailer()?;
	progress.finish();
	eprintln!("Wrote {}", out_path.display());
	Ok(())
}

fn export_mp3(file: &MediaFile, out_path: &Path, args: &Cli) -> anyhow::Result<()> {
	let mut octx = ffmpeg::format::output(out_path)?;

	let (stream_index, desc) = file.stream_descs
		.iter()
		.enumerate()
		.find(|(_, desc)| desc.content.is_audio())
		.ok_or_else(|| anyhow::anyhow!("The input file has no audio stream."))?;

	let mut audio_track = open_audio_track(&mut octx, desc, stream_index as u8, args, ffmpeg::codec::Id::MP3)?;

	octx.write_header()?;

	let progress = indicatif::ProgressBar::new(file.packets.len() as u64)
		.with_style(shared::progress_style());

	for packet in file.packets
		.iter()
		.filter(|packet| packet.stream_id as usize == stream_index)
	{
		if let PacketContent::Audio(sample) = &packet.content {
			audio_track.set_targets(sample);
			audio_track.next_frame_index += 1;

			let ts_end = Duration::from(desc.rate.cast::<u32>() * audio_track.next_frame_index as u32);
			audio_track.render_until(ts_end.as_secs_f64(), &mut octx)?;
		}
		progress.inc(1);
	}

	audio_track.finish(&mut octx)?;
	octx.write_trailer()?;
	progress.finish();
	eprintln!("Wrote {}", out_path.display());
	Ok(())
}
