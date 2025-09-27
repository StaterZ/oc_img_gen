use std::cmp::Ordering;

use itertools::Itertools;
use realfft::RealFftPlanner;

use crate::audio::packet::{Sample, VoiceState};

pub mod packet;

pub struct Config {
	pub analysis_rate: u32,
	pub fft_window_size: usize,
	pub hop_length: usize,
	pub normalize: bool,
	pub num_voices: usize,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			analysis_rate: 22050,
			fft_window_size: 1024,
			hop_length: 512,
			normalize: true,
			num_voices: 8,
		}
	}
}

pub struct VoiceStateFlt {
	volume: f32,
	frequency: f32,
}

struct FrameInstr {
	voices: Vec<VoiceStateFlt>,
	dur_ms: f32,
}

pub fn encode(config: &Config, pcm: &Vec<f32>) -> Vec<Sample> {
	// Prepare FFT
	let mut planner = RealFftPlanner::<f32>::new();
	let r2c = planner.plan_fft_forward(config.fft_window_size);
	let mut fft_in = r2c.make_input_vec();
	let mut fft_out = r2c.make_output_vec();

	// For normalization (optional)
	let mut global_peak = 0f32;

	// Collect windows (frequency+amplitude sets)
	let mut timeline: Vec<FrameInstr> = Vec::new();

	// Window/Hop analysis
	let mut pos = 0usize;
	while pos + config.fft_window_size <= pcm.len() {
		// Copy & window (Hann)
		for i in 0..config.fft_window_size {
			let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / config.fft_window_size as f32).cos();
			fft_in[i] = pcm[pos + i] * w;
		}
		r2c.process(&mut fft_in, &mut fft_out).unwrap();

		// Magnitudes up to Nyquist
		let bin_hz = config.analysis_rate as f32 / config.fft_window_size as f32;
		let mut peaks = fft_out
			.iter()
			.enumerate()
			.map(|(k, c)| VoiceStateFlt {
				volume: (c.re * c.re + c.im * c.im).sqrt(),
				frequency: k as f32 * bin_hz,
			})
			.collect_vec();
		
		// Ignore DC (k=0)
		if !peaks.is_empty() {
			peaks[0].volume = 0.0;
		}

		// Pick top-k non-overlapping peaks (greedy)
		peaks.sort_by(|a, b| b.volume.partial_cmp(&a.volume).unwrap_or(Ordering::Equal));
		let mut chosen = Vec::new();
		let mut used_bins = vec![false; fft_out.len()];
		for peak in peaks.into_iter() {
			if chosen.len() >= config.num_voices { break; }

			let bin = (peak.frequency / bin_hz).round() as isize;
			let guard = 1; // simple local exclusion
			let mut ok = true;
			for off in -guard..=guard {
				let idx = bin + off;
				if idx >= 0 && (idx as usize) < used_bins.len() && used_bins[idx as usize] {
					ok = false;
					break;
				}
			}
			if ok {
				if bin >= 0 && (bin as usize) < used_bins.len() {
					used_bins[bin as usize] = true;
				}
				chosen.push(peak);
			}
		}

		// Track global peak for later normalization
		for local_peak in chosen.iter() {
			global_peak = global_peak.max(local_peak.volume);
		}

		let dur_ms = (config.hop_length as f32 / config.analysis_rate as f32) * 1000.0;
		timeline.push(FrameInstr { voices: chosen, dur_ms });
		pos += config.hop_length;
	}

	// Write output	
	let norm = if config.normalize { global_peak.max(1e-9) } else { 1.0 };
	
	let mut samples = Vec::new();
	for frame in &timeline {
		samples.push(Sample {
			voices: (0..config.num_voices).map(|i| if let Some(voice_state) = frame.voices.get(i) {
				VoiceState {
					volume: ((voice_state.volume / norm).clamp(0.0, 1.0) * (0xff as f32)) as u8,
					frequency: ((voice_state.frequency / 20000.0).clamp(0.0, 1.0) * (0xffff as f32)) as u16,
				}
			} else {
				VoiceState {
					volume: 0,
					frequency: 0, //TODO: don't put frequency in file if volume is 0
				}
			}).collect_vec(),
			duration: (frame.dur_ms.round() as usize).clamp(0, 0xff) as u8,
		});
	}
	samples
}
