//! DFPWM1a, the 1-bit codec that computronics tape drives play.
//! ported from the java version computronics uses (Ben "GreaseMonkey" Russell, public domain)

/// The sample rate a tape drive plays at with speed 1.0. It sends 1500 bytes every 250ms.
pub const TAPE_SAMPLE_RATE: u32 = 48_000;
/// A tape drive plays `round(1500 * speed)` bytes per 250ms, so its sample rate is a multiple of this.
pub const TAPE_SAMPLE_RATE_STEP: u32 = 32;
pub const TAPE_MIN_SPEED: f32 = 0.25;
pub const TAPE_MAX_SPEED: f32 = 2.0;

const RESP_PREC: i32 = 10;
const LPF_STRENGTH: i32 = 140;

#[derive(Default)]
struct Context {
	response: i32,
	level: i32,
	last_bit: bool,
}

impl Context {
	fn update(&mut self, bit: bool) {
		let target = if bit { 127 } else { -128 };
		let mut level = self.level + ((self.response * (target - self.level) + (1 << (RESP_PREC - 1))) >> RESP_PREC);
		if level == self.level && self.level != target {
			level += if bit { 1 } else { -1 };
		}

		let response_target = if bit == self.last_bit { (1 << RESP_PREC) - 1 } else { 0 };
		let mut response = self.response;
		if response != response_target {
			response += if bit == self.last_bit { 1 } else { -1 };
		}
		response = response.max(2 << (RESP_PREC - 8));

		self.response = response;
		self.last_bit = bit;
		self.level = level;
	}
}

/// Packs 8 samples per byte, lowest bit first. The tail is padded with silence.
pub fn encode(samples: &[i8]) -> Vec<u8> {
	let mut ctx = Context::default();
	samples
		.chunks(8)
		.map(|chunk| {
			let mut byte = 0u8;
			for i in 0..8 {
				let sample = chunk.get(i).copied().unwrap_or(0) as i32;
				let bit = sample > ctx.level || (sample == ctx.level && ctx.level == 127);
				byte = (byte >> 1) | if bit { 0x80 } else { 0 };
				ctx.update(bit);
			}
			byte
		})
		.collect()
}

/// Decodes like the tape drive does, with its noise shaping and low-pass filter.
pub fn decode(data: &[u8]) -> Vec<i8> {
	let mut ctx = Context::default();
	let mut last_level = 0;
	let mut lpf_level = 0;
	let mut out = Vec::with_capacity(data.len() * 8);
	for &byte in data {
		for i in 0..8 {
			let bit = (byte >> i) & 1 != 0;
			let last_bit = ctx.last_bit;
			ctx.update(bit);

			let level = (if bit == last_bit { ctx.level } else { (last_level + ctx.level + 1) >> 1 }) as i8 as i32;
			last_level = ctx.level;

			lpf_level += (LPF_STRENGTH * (level - lpf_level) + 0x80) >> 8;
			out.push(lpf_level as i8);
		}
	}
	out
}

/// The tape speed that plays at `sample_rate`.
pub fn tape_speed(sample_rate: u32) -> f32 {
	sample_rate as f32 / TAPE_SAMPLE_RATE as f32
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn round_trip_follows_a_sine() {
		let samples = (0..48_000)
			.map(|i| ((i as f32 / 48_000.0 * 440.0 * std::f32::consts::TAU).sin() * 100.0) as i8)
			.collect::<Vec<_>>();
		let data = encode(&samples);
		assert_eq!(data.len(), samples.len() / 8);

		let decoded = decode(&data);
		let error = samples.iter()
			.zip(&decoded)
			.skip(1000) //let the codec settle
			.map(|(a, b)| (*a as f32 - *b as f32).abs())
			.sum::<f32>() / (samples.len() - 1000) as f32;
		assert!(error < 20.0, "mean error {error}");
	}

	#[test]
	fn pads_the_tail() {
		assert_eq!(encode(&[0; 9]).len(), 2);
	}
}
