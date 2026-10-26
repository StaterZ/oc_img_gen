use std::{collections::VecDeque, marker::ConstParamTy, path::Path, sync::LazyLock};
use deku::prelude::*;
use num::traits::ConstZero;
use ordered_float::OrderedFloat;
use palette::color_difference::ImprovedCiede2000;
use palette::rgb::channels::Argb;
use palette::{Hsv, Lab, Srgb, FromColor};
use szu::iter::RleRun;

use crate::math::*;
use crate::encoder::{
	cli::{Budget, VideoFilter, BrailleStrategy, ColorDiffStrategy},
	media_container::{
		Descriptor as StreamDescriptor,
		Packet,
		PacketContent
	},
	muxer::PacketWriter,
};
use crate::video::braille::Braille;
use crate::video::rgb;
use super::{
	super::{
		braille,
		oc_color::{formatters::{HybridFormatter, Formatter}, PackedColor, PaletteOr},
		Image,
	},
	batcher,
	renderers::{CachedRenderer, StatRenderer, SztRenderer},
	TermFrame,
	machine::Machine,
};

#[derive(DekuWrite, DekuRead, ConstParamTy, PartialEq, Eq)]
#[deku(endian = "little", id_type = "u8")]
pub enum CommandKind {
	#[deku(id = 0x00)] Text,
	#[deku(id = 0x01)] Braille,
}

#[derive(Debug, DekuWrite, DekuRead)]
#[deku(endian = "little")]
pub struct CommandFlags {
	#[deku(bits = 1)] pub has_background: bool,
	#[deku(bits = 1)] pub has_foreground: bool,
	#[deku(bits = 1)] pub is_rle: bool,
	#[deku(bits = 5)] pub len: u8,
}

#[derive(Debug, DekuWrite, DekuRead)]
pub struct Command {
	#[deku(update = "CommandFlags {
		has_background: self.background.is_some(),
		has_foreground: self.foreground.is_some(),
		is_rle: matches!(self.data, CommandData::Rle(_)),
		len: self.data.len() as u8,
	}")]
	pub flags: CommandFlags,
	#[deku(cond = "flags.has_background")] pub background: Option<PackedColor>,
	#[deku(cond = "flags.has_foreground")] pub foreground: Option<PackedColor>,
	pub pos: Point<u8>,
	#[deku(ctx = "&flags")]
	pub data: CommandData,
}

#[derive(Debug, DekuWrite, DekuRead)]
#[deku(ctx = "flags: &CommandFlags", id = "flags.is_rle")]
pub enum CommandData {
	#[deku(id = false)]
	Raw(#[deku(count = "flags.len + 1")] Vec<u8>),
	#[deku(id = true)]
	Rle(#[deku(count = "flags.len + 1")] Vec<RleRun<u8, u8>>),
}

impl CommandData {
	pub fn len(&self) -> usize {
		match self {
			CommandData::Raw(items) => items.len(),
			CommandData::Rle(runs) => runs.len(),
		}
	}
}

impl Command {
	pub const MAX_BRAILLE_COUNT: usize = 1 << (8 - 3);
}

#[derive(DekuWrite, DekuRead)]
#[deku(ctx = "desc: &Descriptor")]
pub struct Frame {
	#[deku(endian = "little", update = "self.commands.len()")] pub commands_len: u16,
	pub command_kind: CommandKind,
	#[deku(count = "commands_len")] pub commands: Vec<u8>,
	//pub commands: Vec<Command>,
}

#[derive(Debug, Clone, DekuWrite, DekuRead)]
pub struct Descriptor {
	pub size: Size<u8>,
}

pub struct VideoEncoder<'a> {
	pub desc: StreamDescriptor<Descriptor>,
	stream_id: u8,
	source_area: Rect<usize>,
	frames: VecDeque<Frame>,
	prev_frame: Option<TermFrame>,
	num_frames_since_emit: usize,
	filter: Option<VideoFilter>,
	braille_strategy: BrailleStrategy,
	color_diff_strategy: ColorDiffStrategy,
	budget: Option<Budget>,
	machine: &'a Machine,
	acceptable_loss: Frac<u32>,
	loss_step: Frac<u32>,
	pub stats: EncodeStats,
	#[cfg(feature = "charts")] chart: charts_rs::MultiChart,
	#[cfg(feature = "charts")] chart_path: std::path::PathBuf,
}

//running totals for the summary printed after encoding, use is cost / budget
#[derive(Default)]
pub struct EncodeStats {
	pub num_frames: usize,
	pub num_dropped: usize,
	pub num_lossy: usize,
	pub num_bytes: usize,
	pub total_loss: f32,
	pub peak_loss: f32,
	pub total_gpu_use: f32,
	pub peak_gpu_use: f32,
	pub total_cpu_use: f32,
	pub peak_cpu_use: f32,
}

//what the last budget step of a frame cost
struct FrameSample {
	set_cost: f32,
	color_cost: f32,
	bitblt_cost: f32,
	gpu_cost: f32,
	gpu_budget: f32,
	cpu_cost: f32,
	cpu_budget: f32,
	loss: f32,
}

#[cfg(feature = "charts")]
fn new_line_chart(title: &str, y_title: &str, series: &[&str]) -> charts_rs::LineChart {
	let mut chart = charts_rs::LineChart::new_with_theme(
		series.iter().map(|name| charts_rs::Series::new(name.to_string(), Vec::new())).collect(),
		Vec::new(),
		charts_rs::THEME_GRAFANA,
	);
	chart.width = 1200.0;
	chart.height = 320.0;
	chart.title.text = title.to_string();
	chart.series.symbol = Some(charts_rs::Symbol::None);
	chart.x_axis.title = "time (s)".to_string();
	chart.y_axis_configs[0].title = Some(y_title.to_string());
	chart
}

static VGA_PALETTE: LazyLock<[Lab; 1 << 3]> = LazyLock::new(|| [
	0x000000,
	0xff0000,
	0x00ff00,
	0xffff00,
	0x0000ff,
	0xff00ff,
	0x00ffff,
	0xffffff,
].map(|c| Lab::from_color(Srgb::<u8>::from_u32::<Argb>(c).into_format::<f32>())));
impl<'a> VideoEncoder<'a> {
	pub fn new(
		desc: StreamDescriptor<Descriptor>,
		stream_id: u8,
		source_area: Rect<usize>,
		filter: Option<VideoFilter>,
		braille_strategy: BrailleStrategy,
		color_diff_strategy: ColorDiffStrategy,
		budget: Option<Budget>,
		machine: &'a Machine,
		acceptable_loss: Frac<u32>,
		loss_step: Frac<u32>,
		out_path: &Path,
	) -> Self {
		//next to the output, named after it
		#[cfg(feature = "charts")] let chart_path = out_path.with_file_name(format!("{}_chart_{}.svg",
			out_path.file_stem().unwrap_or_default().to_string_lossy(),
			desc.name.replace(',', "_"),
		));
		#[cfg(not(feature = "charts"))] let _ = out_path;

		#[cfg(feature = "charts")] let chart = {
			let name: &str = &desc.name;
			let gpu_chart = new_line_chart(&format!("{name} | GPU cost per frame"), "call budget", &["set", "color", "bitblt", "cost", "budget"]);
			let cpu_chart = new_line_chart(&format!("{name} | CPU cost per frame"), "cpu time", &["cost", "budget"]);

			let mut use_chart = new_line_chart(&format!("{name} | Budget use"), "% of budget", &["gpu", "cpu"]);
			use_chart.series_list[0].mark_lines.push(charts_rs::MarkLine {
				category: charts_rs::MarkLineCategory::Value(100.0),
				..Default::default()
			});

			//loss on the left axis, packet size on the right one
			let mut size_chart = new_line_chart(&format!("{name} | Loss & packet size"), "loss %", &["loss", "bytes"]);
			let mut bytes_axis = size_chart.y_axis_configs[0].clone();
			bytes_axis.title = Some("bytes".to_string());
			size_chart.y_axis_configs.push(bytes_axis);
			size_chart.series_list[1].y_axis_index = 1;

			let mut multi_chart = charts_rs::MultiChart::new();
			multi_chart.margin = (10.0).into();
			multi_chart.gap = 20.0;
			multi_chart.align_plots = true;
			multi_chart.background_color = Some((31, 29, 29, 150).into());
			multi_chart.add(charts_rs::ChildChart::Line(gpu_chart, None));
			multi_chart.add(charts_rs::ChildChart::Line(cpu_chart, None));
			multi_chart.add(charts_rs::ChildChart::Line(use_chart, None));
			multi_chart.add(charts_rs::ChildChart::Line(size_chart, None));
			multi_chart
		};

		Self {
			desc,
			stream_id,
			source_area,
			frames: VecDeque::new(),
			prev_frame: None,
			num_frames_since_emit: 0,
			filter,
			braille_strategy,
			color_diff_strategy,
			budget,
			machine,
			acceptable_loss,
			loss_step,
			stats: EncodeStats::default(),
			#[cfg(feature = "charts")] chart,
			#[cfg(feature = "charts")] chart_path,
		}
	}

	pub fn report(&self) -> String {
		let stats = &self.stats;
		let num_emitted = (stats.num_frames - stats.num_dropped).max(1) as f32;
		let percent = |n: usize| 100.0 * n as f32 / stats.num_frames.max(1) as f32;
		let mut line = format!("{:>6} {} @ {:.2}fps | {} frames, {} dropped ({:.1}%), {} lossy ({:.1}%) | {} ({}/frame)",
			&*self.desc.name,
			self.desc.content.size,
			self.desc.rate.inverse().into_flt::<f32>(),
			stats.num_frames,
			stats.num_dropped, percent(stats.num_dropped),
			stats.num_lossy, percent(stats.num_lossy),
			crate::format_bytes(stats.num_bytes),
			crate::format_bytes(stats.num_bytes / stats.num_frames.max(1)),
		);
		if self.budget.is_some() {
			line += &format!(" | loss avg {:.1}% peak {:.1}% | gpu avg {:.0}% peak {:.0}% | cpu avg {:.0}% peak {:.0}%",
				stats.total_loss / num_emitted, stats.peak_loss,
				stats.total_gpu_use / num_emitted, stats.peak_gpu_use,
				stats.total_cpu_use / num_emitted, stats.peak_cpu_use,
			);
		}
		line
	}

	//short live stats for the progress bar
	pub fn progress_message(&self) -> String {
		let stats = &self.stats;
		let num_emitted = (stats.num_frames - stats.num_dropped).max(1) as f32;
		let mut message = format!("{} | {} dropped", crate::format_bytes(stats.num_bytes), stats.num_dropped);
		if self.budget.is_some() {
			message += &format!(" | loss {:.1}% | gpu {:.0}%", stats.total_loss / num_emitted, stats.total_gpu_use / num_emitted);
		}
		message
	}

	pub fn process(&mut self, img: &Image<Srgb<u8>>, formatter: &HybridFormatter) {
		let img = crate::stage("Stream | Preamble  | Crop", || img.crop(&self.source_area));
		debug_assert_eq!(img.size(), self.desc.content.size.cast::<usize>() * braille::SIZE);

		let img = if let Some(filter) = self.filter {
			Self::apply_filter(filter, img)
		} else {
			crate::stage("Stream | Process   | ToLab", || img
				.into_iter()
				.map(|p| Lab::from_color(p.into_format::<f32>()))
				.collect())
		};

		// let mut img = image::RgbImage::new::from_vec(
		// 	img.size().w as u32,
		// 	img.size().h as u32,
		// 	img.into_buffer(),
		// );
		// image::imageops::colorops::dither(&mut img, formatter);

		// let color_diff = match self.color_diff_strategy {
		// 	ColorDiffStrategy::Fast => |a: Lab, b| a.distance_squared(b),
		// 	ColorDiffStrategy::ImprovedCiede2000 => |a: Lab, b| a.improved_difference(b),
		// };

		// let braille_from_pixels = match self.braille_strategy {
		// 	BrailleStrategy::CentroidCohesion => |cluster| Braille::from_pixels_centroid_cohesion(&cluster, color_diff),
		// 	BrailleStrategy::PolarPair => |cluster| Braille::from_pixels_polar_pair(&cluster, color_diff),
		// };

		let deflate = |braille: Braille<Lab>| braille.map(|p| formatter.deflate(PaletteOr::NonPalette(*p)));
		let braille_strategy = self.braille_strategy;
		let braille_from_pixels = |cluster| match braille_strategy {
			BrailleStrategy::CentroidCohesion => deflate(Braille::from_pixels_centroid_cohesion(&cluster)),
			BrailleStrategy::PolarPair => deflate(Braille::from_pixels_polar_pair(&cluster)),
			BrailleStrategy::PalettePair => Braille::from_pixels_palette_pair(&cluster, formatter),
			BrailleStrategy::PaletteDither => Braille::from_pixels_palette_dither(&cluster, formatter),
		};

		const UBER_MODE: bool = true;
		let img = if UBER_MODE {
			crate::stage("Stream | Process   | Uber", || {
				braille::as_braille(&img, braille_from_pixels)
					.map(|braille| braille.into())
					.collect()
			})
		} else {
			//img.debug_export("raw".into()).unwrap();
			//let img2 = crate::stage("Stream | Process   | Deflate", || img.iter().map(|p| formatter.deflate(PaletteOr::NonPalette(*p))).collect());
			//let img2 = crate::stage("Stream | Process   | Inflate", || img2.into_iter().map(|p| formatter.inflate(p)).collect());
			//img2.debug_export("raw_deflate".into()).unwrap();

			let img: Image<_> = crate::stage("Stream | Process   | Braille", || braille::as_braille(&img, braille_from_pixels).collect());
			crate::stage("Stream | Process   | Into Text", || img.into_iter().map(|braille| braille.into()).collect())
		};

		//std::fs::write("code_out.lua", super::code_gen(&img, None, self.acceptable_loss, formatter)).unwrap();
		crate::stage("Stream | Postamble | Cmd Gen", || self.push_frame::<{ CommandKind::Braille }>(img, self.acceptable_loss, formatter));
	}

	fn apply_filter(filter: VideoFilter, img: Image<Srgb<u8>>) -> Image<Lab> {
		match filter {
			VideoFilter::Monochrome => crate::stage("Stream | Process   | Monochrome", || img.into_iter().map(|p| {
				let p = Lab::from_color(p.into_format::<f32>());
				if p.improved_difference(*rgb::WHITE_LAB) < p.improved_difference(*rgb::BLACK_LAB) { *rgb::WHITE_LAB } else { *rgb::BLACK_LAB }
			}).collect()),
			VideoFilter::Grayscale => crate::stage("Stream | Process   | Grayscale", || img.into_iter().map(|p| {
				let p = Lab::from_color(p.into_format::<f32>());
				Lab::new(p.l, 0.0, 0.0)
			}).collect()),
			VideoFilter::Vga => crate::stage("Stream | Process   | VGA", || img.into_iter().map(|p| {
				let mut hsv = Hsv::from_color(p.into_format::<f32>());
				hsv.saturation = 1.0;
				let p = Lab::from_color(hsv);
				*VGA_PALETTE.iter().min_by_key(|c| OrderedFloat(p.improved_difference(**c))).unwrap() //SAFETY: unwrap safe due to VGA_PALETTE.len > 0
			}).collect()),
			VideoFilter::Hsv => crate::stage("Stream | Process   | HSV", || img.into_iter().map(|p| {
				let mut hsv = Hsv::from_color(p.into_format::<f32>());
				hsv.saturation = (hsv.saturation * 6.0).round() / 6.0;
				hsv.value = (hsv.value * 4.0).round() / 4.0;
				Lab::from_color(hsv)
			}).collect()),
		}
	}

	fn push_frame<const CMD_KIND: CommandKind>(&mut self, frame: TermFrame, acceptable_loss: Frac<u32>, formatter: &impl Formatter) {
		let mut loss = 0.into();

		self.num_frames_since_emit += 1;
		let mut losses = None; //per pixel loss against prev_frame, only built once the loss rises above zero
		let (frame, sample, is_dropped) = loop {
			if loss!= Frac::ZERO && losses.is_none() && let Some(prev_frame) = self.prev_frame.as_ref() {
				losses = Some(Image::with_buffer(frame.size(), frame.buffer()
					.iter()
					.zip(prev_frame.buffer())
					.map(|(char, char_prev)| char.compute_loss(char_prev, formatter))
					.collect()));
			}

			let mut renderer = CachedRenderer::new(StatRenderer::new(SztRenderer::<CMD_KIND>::new()));
			let mut work_frame = self.prev_frame.clone();
			batcher::draw(&mut renderer, &frame, &mut work_frame, losses.as_ref(), Command::MAX_BRAILLE_COUNT, loss, formatter);
			let renderer = renderer.into_inner();
			let mut stats = renderer.get_stats();
			let gpu_cost = match self.budget {
				Some(Budget::Direct) => {
					//every set and color call goes straight to the screen and draws from the budget
					//the szt format sets the resolution once before playback, never per frame
					let num_calls = stats.num_color_commands + stats.num_set_commands;
					if num_calls == 0 {
						Frac::ZERO
					} else {
						let avg_call_cost = (stats.get_set_color_cost(&self.machine) + stats.get_set_cost(&self.machine)) / num_calls;
						self.machine.effective_call_cost(avg_call_cost) * num_calls
					}
				},
				Some(Budget::Buffered) => {
					//set and color calls into a vram buffer cost no budget, only presenting the frame to the screen does
					//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/component/GraphicsCard.scala#L100-L107
					//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/component/GraphicsCard.scala#L309-L311
					if stats.num_set_commands > 0 {
						stats.num_bitblt_pixels += frame.size().area();
						self.machine.present_cost(frame.size())
					} else {
						Frac::ZERO
					}
				},
				None => Frac::ZERO,
			};
			let cpu_pixels_per_sec = Frac::new(1, 120000); //rough experimentally measured speed of lua playback engine
			let cpu_cost = cpu_pixels_per_sec * stats.num_set_pixels;

			let tick_rate = 20;
			let time_since_emit = self.desc.rate.cast::<usize>() * self.num_frames_since_emit;
			let gpu_budget = (self.machine.call_budget * tick_rate) * time_since_emit;
			let cpu_budget = self.machine.cpu_speed * time_since_emit;

			let sample = FrameSample {
				set_cost: stats.get_set_cost(&self.machine).into_flt(),
				color_cost: stats.get_set_color_cost(&self.machine).into_flt(),
				bitblt_cost: stats.get_bitblt_cost(&self.machine).into_flt(),
				gpu_cost: gpu_cost.into_flt(),
				gpu_budget: gpu_budget.into_flt(),
				cpu_cost: cpu_cost.into_flt(),
				cpu_budget: cpu_budget.into_flt(),
				loss: loss.into_flt::<f32>() * 100.0,
			};

			loss += self.loss_step;
			let has_budget = self.budget.is_some();
			let can_afford = gpu_cost <= gpu_budget && cpu_cost <= cpu_budget;
			let is_loss_acceptable = loss <= acceptable_loss;
			if !has_budget || can_afford || !is_loss_acceptable {
				if has_budget && !can_afford && !is_loss_acceptable {
					break (Frame {
						commands_len: 0,
						command_kind: CMD_KIND,
						commands: Vec::new(),
					}, sample, true);
				}

				self.num_frames_since_emit = 0;
				if work_frame.is_none() {
					self.prev_frame = Some(frame);
				} else {
					self.prev_frame = work_frame;
				}
				break (renderer.into_inner().build(), sample, false);
			}
		};

		self.record(&frame, &sample, is_dropped);
		self.frames.push_back(frame);
	}

	fn record(&mut self, frame: &Frame, sample: &FrameSample, is_dropped: bool) {
		let use_of = |cost: f32, budget: f32| if budget > 0.0 { 100.0 * cost / budget } else { 0.0 };
		let gpu_use = use_of(sample.gpu_cost, sample.gpu_budget);
		let cpu_use = use_of(sample.cpu_cost, sample.cpu_budget);
		let num_bytes = 1 + 2 + 1 + frame.commands.len(); //stream id, commands_len, command_kind, commands

		#[cfg(feature = "charts")] {
			let time = (self.desc.rate.cast::<usize>() * self.stats.num_frames).into_flt::<f64>();
			let mut push = |chart: usize, values: &[Option<f32>]| {
				let charts_rs::ChildChart::Line(chart, _) = &mut self.chart.charts[chart] else { unreachable!() };
				chart.x_axis.values.push(time);
				for (series, value) in chart.series_list.iter_mut().zip(values) {
					series.data.push(*value);
				}
			};
			push(0, &[Some(sample.set_cost), Some(sample.color_cost), Some(sample.bitblt_cost), Some(sample.gpu_cost), Some(sample.gpu_budget)]);
			push(1, &[Some(sample.cpu_cost), Some(sample.cpu_budget)]);
			push(2, &[Some(gpu_use), Some(cpu_use)]);
			push(3, &[Some(sample.loss), Some(num_bytes as f32)]); //dropped frames show the loss they gave up at
		}

		let stats = &mut self.stats;
		stats.num_frames += 1;
		stats.num_bytes += num_bytes;
		if is_dropped {
			stats.num_dropped += 1;
			return;
		}
		if sample.loss > 0.0 { stats.num_lossy += 1; }
		stats.total_loss += sample.loss;
		stats.peak_loss = stats.peak_loss.max(sample.loss);
		stats.total_gpu_use += gpu_use;
		stats.peak_gpu_use = stats.peak_gpu_use.max(gpu_use);
		stats.total_cpu_use += cpu_use;
		stats.peak_cpu_use = stats.peak_cpu_use.max(cpu_use);
	}
}

impl<'a> Drop for VideoEncoder<'a> {
	fn drop(&mut self) {
		#[cfg(feature = "charts")] {
			std::fs::write(&self.chart_path, self.chart.svg().unwrap()).unwrap();
			eprintln!("chart: {}", self.chart_path.display());
		}
	}
}

impl<'a> PacketWriter for VideoEncoder<'a> {
	fn get_next_packet_end_time(&self) -> Option<Frac<u64>> {
		(!self.frames.is_empty()).then_some(Frac::from(self.desc.num_packets as u64 + 1) * self.desc.rate.cast::<u64>())
	}

	fn get_next_packet(&mut self) -> Option<Packet> {
		self.frames.pop_front().map(|frame| {
			self.desc.num_packets += 1;
			Packet {
				stream_id: self.stream_id,
				content: PacketContent::Video(frame),
			}
		})
	}
}
