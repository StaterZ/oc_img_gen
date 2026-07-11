use grid::Grid;
use itertools::Itertools;
use lapjv::lapjv;

use crate::audio::packet::VoiceState;

pub mod packet;
pub mod audio_reader;

pub struct Config {
	pub name: String,
	pub analysis_rate: u32,
	pub fft_window_size: usize,
	pub hop_length: usize,
	pub guard: isize,
	pub normalize: bool,
	pub num_voices: usize,
}

#[derive(Clone, Copy, Default)]
pub struct VoiceStateFlt {
	pub volume: f32,
	pub frequency: f32,
}

pub fn encode(master_volume: f32, timeline: Grid<VoiceStateFlt>) -> Grid<VoiceState> {
	timeline.map(|voice_state| VoiceState {
		volume: ((voice_state.volume * master_volume).clamp(0.0, 1.0) * (u8::MAX as f32)) as u8,
		frequency: ((voice_state.frequency / 20000.0).clamp(0.0, 1.0) * (u16::MAX as f32)) as u16,
	})
}

pub fn optimize_channel_jumping(timeline: &mut Grid<VoiceStateFlt>) {
	for t in 0..(timeline.rows() - 1) {
		let n = timeline.cols();
		let mut cost = lapjv::Matrix::<f32>::zeros((n, n));
		for i in 0..n {
			for j in 0..n {
				let voice_a = &timeline[(t, i)];
				let voice_b = &timeline[(t + 1, j)];
				let error = (voice_a.frequency - voice_b.frequency).abs();
				let criticality = (voice_a.volume + voice_b.volume) * 0.5;
				cost[(i, j)] = error * criticality;
			}
		}

		let (assign, _) = lapjv(&cost).expect("assignment failed");

		// Reorder according to assignment
		let row = assign.into_iter().map(|j| timeline[(t + 1, j)]).collect_vec();
		for (i, cell) in row.into_iter().enumerate() {
			timeline[(t + 1, i)] = cell;
		}
	}
}
