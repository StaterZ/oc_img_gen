use std::{collections::VecDeque, marker::ConstParamTy};
use deku::prelude::*;
use num_traits::ConstZero;

use crate::cli::{Budget, VideoFilter};
use crate::math::*;
use crate::encoder::{
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
	BrailleFrame,
	TermFrame,
	Machine,
};

#[derive(DekuWrite, ConstParamTy, PartialEq, Eq)]
#[deku(endian = "little", id_type = "u8")]
pub enum CommandKind {
	#[deku(id = 0x00)] Text,
	#[deku(id = 0x01)] Braille,
}

#[derive(DekuWrite)]
#[deku(endian = "little")]
pub struct CommandFlags {
	#[deku(bits = 1)] pub has_background: bool,
	#[deku(bits = 1)] pub has_foreground: bool,
	#[deku(bits = 6)] pub len: u8,
}

#[derive(DekuWrite)]
pub struct Command {
	pub flags: CommandFlags,
	#[deku(cond = "flags.has_background")] pub background: Option<PackedColor>,
	#[deku(cond = "flags.has_foreground")] pub foreground: Option<PackedColor>,
	pub pos: Point<u8>,
	#[deku(count = "flags.len")]
	pub braille: Vec<u8>,
}

#[derive(DekuWrite)]
#[deku(ctx = "desc: &Descriptor")]
pub struct Frame {
	#[deku(endian = "little", update = "self.commands.len()")] pub commands_len: u16,
	pub command_kind: CommandKind,
	#[deku(count = "commands_len")] pub commands: Vec<u8>,
	//pub commands: Vec<Command>,
}

#[derive(Clone, DekuWrite)]
pub struct Descriptor {
	pub size: Size<u8>,
}

pub struct VideoEncoder {
	pub desc: StreamDescriptor<Descriptor>,
	stream_id: u8,
	source_area: Option<Rect<usize>>,
	frames: VecDeque<Frame>,
	prev_frame: Option<TermFrame>,
	num_frames_since_emit: usize,
	filter: Option<VideoFilter>,
	budget: Option<Budget>,
	#[cfg(feature = "charts")] chart: charts_rs::MultiChart,
}

impl VideoEncoder {
	pub fn new(
		desc: StreamDescriptor<Descriptor>,
		stream_id: u8,
		source_area: Option<Rect<usize>>,
		filter: Option<VideoFilter>,
		budget: Option<Budget>,
	) -> Self {
		#[cfg(feature = "charts")] let chart = {
			let mut gpu_chart = charts_rs::LineChart::new_with_theme(vec![
				charts_rs::Series::new("budget".to_string(), Vec::new()),
				charts_rs::Series::new("cost".to_string(), Vec::new()),
				charts_rs::Series::new("color".to_string(), Vec::new()),
				charts_rs::Series::new("set".to_string(), Vec::new()),
				charts_rs::Series::new("bitblt".to_string(), Vec::new()),
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
			#[cfg(feature = "charts")] chart,
		}
	}
	
	pub fn process(&mut self, img: &Image<RGB8>, formatter: &HybridFormatter) {
		let img = match &self.source_area {
			Some(source_area) => crate::stage("Stream | Preamble  | Crop", || img.crop(source_area)),
			None => img.clone(), //TODO: PERF
		};

		let img = if let Some(filter) = self.filter {
			match filter {
				VideoFilter::Monochrome => crate::stage("Stream | Process   | Black&White", || img.map(|p| {
					const BLACK: RGB8 = RGB8::new(0x000000);
					const WHITE: RGB8 = RGB8::new(0xffffff);
					if p.perceptual_delta(WHITE) < p.perceptual_delta(BLACK) { WHITE } else { BLACK }
				})),
				VideoFilter::Grayscale => crate::stage("Stream | Process   | Grayscale", || img.map(|p| {
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
				})),
			}
		} else { img };

		// let img = crate::stage("Stream | Process   | Deflate", || img.map(|p| formatter.deflate(PaletteOr::NonPalette(*p))));
		// let img = crate::stage("Stream | Process   | Inflate", || img.map(|p| formatter.inflate(*p)));

		let img = crate::stage("Stream | Process   | Braille", || braille::as_braille(&img));
		let img = crate::stage("Stream | Process   | B_Deflate ", || img.map(|braille| braille.map(|p| formatter.deflate(PaletteOr::NonPalette(*p)))));
		
		crate::stage("Stream | Postamble | Cmd Gen", || self.push_frame_braille(&img));
		//println!("{}", cmd::code_gen(&img.map(|braille| braille.into()), None, &formatter));
	}

	fn push_frame_text(&mut self, frame: TermFrame) {
		self.push_frame::<{ CommandKind::Text }>(frame);
	}

	fn push_frame_braille(&mut self, frame: &BrailleFrame) {
		self.push_frame::<{ CommandKind::Braille }>(frame.map(|braille| braille.into()));
	}

	fn push_frame<const CMD_KIND: CommandKind>(&mut self, frame: TermFrame) {
		let mut renderer = CachedRenderer::new(StatRenderer::new(SztRenderer::<CMD_KIND>::new()));
		batcher::draw(&mut renderer, &frame, self.prev_frame.as_ref());
		let renderer = renderer.into_inner();
		let mut stats = renderer.get_stats();
		let machine = Machine::T3;
		let gpu_cost = match self.budget {
			Some(Budget::Direct) => stats.get_cost(&machine),
			Some(Budget::Buffered) => {
				if stats.num_set_commands > 0 {
					stats.num_bitblt_pixels += frame.size().area();
				};
				let mut temp_stats = stats;
				temp_stats.num_set_commands = 0;
				temp_stats.get_cost(&machine)
			}
			None => Frac::ZERO,
		};
		let cpu_cost = Frac::new(stats.num_set_pixels, 15000);

		self.num_frames_since_emit += 1;
		let tick_rate = 20;
		let gpu_budget = (machine.call_budget * tick_rate) * self.desc.rate.cast::<usize>() * self.num_frames_since_emit;
		let cpu_budget = machine.cpu_speed * self.desc.rate.cast::<usize>() * self.num_frames_since_emit;

		#[cfg(feature = "charts")] {
			let timestamp = format!("{:.2}s", (self.desc.rate.cast::<usize>() * self.frames.len()).into_flt::<f32>());

			let charts_rs::ChildChart::Line(gpu_chart, _) = &mut self.chart.charts[0] else { unreachable!() };
			gpu_chart.x_axis_data.push(timestamp.clone());
			gpu_chart.series_list[0].data.push(gpu_budget.into_flt());
			gpu_chart.series_list[1].data.push(gpu_cost.into_flt());
			gpu_chart.series_list[2].data.push(stats.get_set_color_cost(&machine).into_flt());
			gpu_chart.series_list[3].data.push(stats.get_set_cost(&machine).into_flt());
			gpu_chart.series_list[4].data.push(stats.get_bitblt_cost(&machine).into_flt());

			let charts_rs::ChildChart::Line(cpu_chart, _) = &mut self.chart.charts[1] else { unreachable!() };
			cpu_chart.x_axis_data.push(timestamp.clone());
			cpu_chart.series_list[0].data.push(cpu_budget.into_flt());
			cpu_chart.series_list[1].data.push(cpu_cost.into_flt());
		}

		let mut frame = if gpu_cost <= gpu_budget && cpu_cost <= cpu_budget {
			self.num_frames_since_emit = 0;
			self.prev_frame = Some(frame);
			renderer.into_inner().build()
		} else {
			Frame { //TODO: remove this terrible by moving from frame-rate-based format to timestamped frames
				commands_len: 0,
				command_kind: CMD_KIND,
				commands: Vec::new(),
			}
		};
		frame.update().unwrap();
		self.frames.push_back(frame);
	}
}

impl Drop for VideoEncoder {
	fn drop(&mut self) {
		#[cfg(feature = "charts")] std::fs::write("chart.svg", self.chart.svg().unwrap()).unwrap();
	}
}

impl PacketWriter for VideoEncoder {
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
