use std::{collections::VecDeque, marker::ConstParamTy};
use deku::prelude::*;
use num_traits::ConstZero;

use crate::math::*;
use crate::encoder::{
	cli::{Budget, VideoFilter},
	media_container::{
		Descriptor as StreamDescriptor,
		Packet,
		PacketContent
	},
	muxer::PacketWriter,
};
use super::{
	super::{
		braille,
		oc_color::{formatters::{HybridFormatter, Formatter}, PackedColor, RGB8, PaletteOr},
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
	#[deku(bits = 6)] pub len: u8,
}

#[derive(Debug, DekuWrite, DekuRead)]
pub struct Command {
	pub flags: CommandFlags,
	#[deku(cond = "flags.has_background")] pub background: Option<PackedColor>,
	#[deku(cond = "flags.has_foreground")] pub foreground: Option<PackedColor>,
	pub pos: Point<u8>,
	#[deku(count = "flags.len + 1")]
	pub braille: Vec<u8>,
}

impl Command {
	pub const MAX_COMMAND_BYTES: usize = 1 << (8 - 2);
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
	budget: Option<Budget>,
	machine: &'a Machine,
	acceptable_loss: Frac<u64>,
	#[cfg(feature = "charts")] chart: charts_rs::MultiChart,
}

impl<'a> VideoEncoder<'a> {
	pub fn new(
		desc: StreamDescriptor<Descriptor>,
		stream_id: u8,
		source_area: Rect<usize>,
		filter: Option<VideoFilter>,
		budget: Option<Budget>,
		machine: &'a Machine,
		acceptable_loss: Frac<u64>,
	) -> Self {
		#[cfg(feature = "charts")] let chart = {
			let mut gpu_chart = charts_rs::LineChart::new_with_theme(vec![
				charts_rs::Series::new("budget".to_string(), Vec::new()),
				charts_rs::Series::new("cost".to_string(), Vec::new()),
				charts_rs::Series::new("color".to_string(), Vec::new()),
				charts_rs::Series::new("set".to_string(), Vec::new()),
				charts_rs::Series::new("bitblt".to_string(), Vec::new()),
				charts_rs::Series::new("loss".to_string(), Vec::new()),
			], Vec::new(), charts_rs::THEME_GRAFANA);
			gpu_chart.series_symbol = Some(charts_rs::Symbol::None);

			let mut cpu_chart = charts_rs::LineChart::new_with_theme(vec![
				charts_rs::Series::new("budget".to_string(), Vec::new()),
				charts_rs::Series::new("cost".to_string(), Vec::new()),
			], Vec::new(), charts_rs::THEME_GRAFANA);
			cpu_chart.series_symbol = Some(charts_rs::Symbol::None);
			
			
			let mut multi_chart = charts_rs::MultiChart::new();
			multi_chart.margin = (10.0).into();
			multi_chart.background_color = Some((31, 29, 29, 150).into());
			multi_chart.add(charts_rs::ChildChart::Line(gpu_chart, None));
			multi_chart.add(charts_rs::ChildChart::Line(cpu_chart, None));
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
			budget,
			machine,
			acceptable_loss,
			#[cfg(feature = "charts")] chart,
		}
	}
	
	pub fn process(&mut self, img: &Image<RGB8>, formatter: &HybridFormatter) {
		let img = crate::stage("Stream | Preamble  | Crop", || img.crop(&self.source_area));
		debug_assert_eq!(img.size(), self.desc.content.size.cast::<usize>() * braille::SIZE);

		let img = if let Some(filter) = self.filter {
			match filter {
				VideoFilter::Monochrome => crate::stage("Stream | Process   | Monochrome", || img.into_iter().map(|p| {
					if p.perceptual_delta(RGB8::WHITE) < p.perceptual_delta(RGB8::BLACK) { RGB8::WHITE } else { RGB8::BLACK }
				}).collect()),
				VideoFilter::Grayscale => crate::stage("Stream | Process   | Grayscale", || img.into_iter().map(|p| {
					fn srgb_to_linear(u: f64) -> f64 {
						if u <= 0.04045 {
							u / 12.92
						} else {
							((u + 0.055) / 1.055).powf(2.4)
						}
					}

					fn linear_to_srgb(v: f64) -> f64 {
						if v <= 0.0031308 {
							12.92 * v
						} else {
							1.055 * v.powf(1.0 / 2.4) - 0.055
						}
					}
				
					let r_lin = srgb_to_linear(p.r as f64 / 255.0);
					let g_lin = srgb_to_linear(p.g as f64 / 255.0);
					let b_lin = srgb_to_linear(p.b as f64 / 255.0);

					// Rec. 709 luminance
					let y_lin = 0.2126 * r_lin + 0.7152 * g_lin + 0.0722 * b_lin;

					// Convert back to sRGB
					let y_srgb = linear_to_srgb(y_lin);

					let luminance = (y_srgb * 255.0).round().clamp(0.0, 255.0) as u8;
					RGB8 {
						r: luminance,
						g: luminance,
						b: luminance,
					}
				}).collect()),
				VideoFilter::Vga => crate::stage("Stream | Process   | VGA", || img.into_iter().map(|p| {
					const PALETTE: [RGB8; 8] = [
						RGB8::new(0x000000),
						RGB8::new(0xff0000),
						RGB8::new(0x00ff00),
						RGB8::new(0xffff00),
						RGB8::new(0x0000ff),
						RGB8::new(0xff00ff),
						RGB8::new(0x00ffff),
						RGB8::new(0xffffff),
					];
					use palette::*;
					let srgb = Srgb::new(p.r, p.g, p.b).into_format::<f32>();
					let mut hsv = Hsv::from_color(srgb);
					hsv.saturation = 1.0;
					let srgb = Srgb::from_color(hsv).into_format::<u8>();
					let p = RGB8 { r: srgb.red, g: srgb.green, b: srgb.blue };
					*PALETTE.iter().min_by_key(|c| p.perceptual_delta(**c)).unwrap() //SAFETY: unwrap safe due to PALETTE.len > 0
				}).collect()),
				VideoFilter::Hsv => crate::stage("Stream | Process   | VGA", || img.into_iter().map(|p| {
					use palette::*;
					let srgb = Srgb::new(p.r, p.g, p.b).into_format::<f32>();
					let mut hsv = Hsv::from_color(srgb);
					hsv.saturation = (hsv.saturation * 6.0).round() / 6.0;
					hsv.value = (hsv.value * 4.0).round() / 4.0;
					let srgb = Srgb::from_color(hsv).into_format::<u8>();
					RGB8 { r: srgb.red, g: srgb.green, b: srgb.blue }
				}).collect()),
			}
		} else { img };

		// let img = crate::stage("Stream | Process   | Deflate", || img.map(|p| formatter.deflate(PaletteOr::NonPalette(*p))));
		// let img = crate::stage("Stream | Process   | Inflate", || img.map(|p| formatter.inflate(*p)));

		// let img = crate::stage("Stream | Process   | Braille", || braille::as_braille(&img).collect());
		// let img = crate::stage("Stream | Process   | Deflate", || img
		// 	.into_iter()
		// 	.map(|braille| braille.map(|p| formatter.deflate(PaletteOr::NonPalette(*p))))
		// 	.collect());
		// let img = crate::stage("Stream | Process   | Into Text", || img.into_iter().map(|braille| braille.into()).collect());

		let img = crate::stage("Stream | Process   | Uber", || {
			braille::as_braille(&img)
				.map(|braille| braille.map(|p| formatter.deflate(PaletteOr::NonPalette(*p))))
				.map(|braille| braille.into())
				.collect()
		});
		
		//std::fs::write("code_out.lua", super::code_gen(&img, None, self.acceptable_loss, formatter)).unwrap();
		crate::stage("Stream | Postamble | Cmd Gen", || self.push_frame::<{ CommandKind::Braille }>(img, self.acceptable_loss, formatter));
	}

	fn push_frame<const CMD_KIND: CommandKind>(&mut self, frame: TermFrame, acceptable_loss: Frac<u64>, formatter: &impl Formatter) {
		let loss_step = Frac::new(1, 100);
		let mut loss = 0.into();

		self.num_frames_since_emit += 1;
		let frame = loop {
			let mut renderer = CachedRenderer::new(StatRenderer::new(SztRenderer::<CMD_KIND>::new()));
			let mut work_frame = self.prev_frame.clone();
			batcher::draw(&mut renderer, &frame, &mut work_frame, Command::MAX_COMMAND_BYTES, loss, formatter); //todo: acceptable_loss treated as loss here
			let renderer = renderer.into_inner();
			let mut stats = renderer.get_stats();
			let gpu_cost = match self.budget {
				Some(Budget::Direct) => stats.get_cost(&self.machine),
				Some(Budget::Buffered) => {
					if stats.num_set_commands > 0 {
						stats.num_bitblt_pixels += frame.size().area();
					};
					let mut temp_stats = stats;
					temp_stats.num_set_commands = 0;
					temp_stats.get_cost(&self.machine)
				},
				None => Frac::ZERO,
			};
			let cpu_cost = Frac::new(stats.num_set_pixels, 15000);

			let tick_rate = 20;
			let gpu_budget = (self.machine.call_budget * tick_rate) * self.desc.rate.cast::<usize>() * self.num_frames_since_emit;
			let cpu_budget = self.machine.cpu_speed * self.desc.rate.cast::<usize>() * self.num_frames_since_emit;

			#[cfg(feature = "charts")] {
				let timestamp = format!("{:.2}s", (self.desc.rate.cast::<usize>() * self.frames.len()).into_flt::<f32>());

				let charts_rs::ChildChart::Line(gpu_chart, _) = &mut self.chart.charts[0] else { unreachable!() };
				gpu_chart.x_axis_data.push(timestamp.clone());
				gpu_chart.series_list[0].data.push(gpu_budget.into_flt());
				gpu_chart.series_list[1].data.push(gpu_cost.into_flt());
				gpu_chart.series_list[2].data.push(stats.get_set_color_cost(&self.machine).into_flt());
				gpu_chart.series_list[3].data.push(stats.get_set_cost(&self.machine).into_flt());
				gpu_chart.series_list[4].data.push(stats.get_bitblt_cost(&self.machine).into_flt());
				gpu_chart.series_list[5].data.push(loss.into_flt());

				let charts_rs::ChildChart::Line(cpu_chart, _) = &mut self.chart.charts[1] else { unreachable!() };
				cpu_chart.x_axis_data.push(timestamp.clone());
				cpu_chart.series_list[0].data.push(cpu_budget.into_flt());
				cpu_chart.series_list[1].data.push(cpu_cost.into_flt());
			}

			loss += loss_step;
			if self.budget == None || loss > acceptable_loss || gpu_cost <= gpu_budget && cpu_cost <= cpu_budget {
				if self.budget.is_some() && loss == 0.into() {
					break Frame { //TODO: remove this terrible by moving from frame-rate-based format to timestamped frames
						commands_len: 0,
						command_kind: CMD_KIND,
						commands: Vec::new(),
					};
				}

				self.num_frames_since_emit = 0;
				if work_frame.is_none() {
					self.prev_frame = Some(frame);
				} else {
					self.prev_frame = work_frame;
				}
				break renderer.into_inner().build();
			}
		};

		self.frames.push_back(frame);
	}
}

impl<'a> Drop for VideoEncoder<'a> {
	fn drop(&mut self) {
		#[cfg(feature = "charts")] std::fs::write("chart.svg", self.chart.svg().unwrap()).unwrap();
	}
}

impl<'a> PacketWriter for VideoEncoder<'a> {
	fn get_next_packet_time(&self) -> Option<Frac<u64>> {
		(!self.frames.is_empty()).then_some(Frac::from(self.desc.num_packets as u64) * self.desc.rate.cast::<u64>())
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
