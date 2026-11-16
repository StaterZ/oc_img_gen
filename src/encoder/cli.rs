use std::{fmt::Display, num::ParseIntError, ops::Deref, path::{Path, PathBuf}, str::FromStr, time::Duration};
use clap::{ArgAction, ArgMatches, Args, FromArgMatches, Parser, ValueEnum, ValueHint};
use itertools::Itertools;
use num::traits::{ConstOne, ConstZero};
use palette::{Srgb, rgb::channels::Argb};

use crate::{
	EXT, audio::{Mode as AudioMode, dfpwm}, encoder::{
		AudioConfig, EncoderConfig, VideoConfig, VideoDescData,
		track_spec::{AudioSpec, Offset, PcmSpec, TracksConfig, VideoSpec, default_name},
	}, math::{Frac, Point, Rect, Size}, video::{self, cmd::machine::{Machine, Tier}}
};

pub fn process_args(mut args: Cli) -> anyhow::Result<EncoderConfig> {
	
	let out_path = match args.out_path.take() {
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

	let mut video_specs = std::mem::take(&mut args.tracks.video);
	let mut audio_specs = std::mem::take(&mut args.tracks.audio);
	let mut pcm_specs = std::mem::take(&mut args.tracks.pcm);

	//the tracks from the config file come after the tracks from the flags
	if let Some(path) = &args.tracks_config {
		let config = TracksConfig::read(path)?;
		video_specs.extend(config.video);
		audio_specs.extend(config.audio);
		pcm_specs.extend(config.pcm);
	}

	let machine = args.tier.into();
	let video_config = build_video_config(*args.fill_color, &video_specs, &machine, &args.in_path)?;
	let audio_configs = build_audio_configs(&AudioOpts::default(), &audio_specs, &pcm_specs)?;
	if video_config.is_none() && audio_configs.is_empty() {
		anyhow::bail!("there are no tracks to encode. add one with --video, --audio or --pcm");
	}
	eprintln!("V:{} | A:{}", video_config.as_ref().map_or(0, |video_config| video_config.track_descs_data.len()), audio_configs.len());
	let settings = describe_settings(&args, video_config.as_ref(), &audio_configs);
	
	Ok(EncoderConfig {
		settings,
		in_path: args.in_path,
		out_path,
		range: args.begin..=args.end,
		machine,
		video: video_config,
		audio: audio_configs,
	})
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackKind {
	Video,
	Audio,
	Pcm,
}

impl TrackKind {
	fn flag(self) -> &'static str {
		match self {
			TrackKind::Video => "--video",
			TrackKind::Audio => "--audio",
			TrackKind::Pcm => "--pcm",
		}
	}
}

/// the track flags (--video, --audio, --pcm) in the order they are on the command line
struct TrackFlags<'a> {
	matches: &'a ArgMatches,
	/// the position of each track flag, and its kind
	tracks: Vec<(usize, TrackKind)>,
}

impl<'a> TrackFlags<'a> {
	fn new(matches: &'a ArgMatches) -> Self {
		let mut tracks = [("video_tracks", TrackKind::Video), ("audio_tracks", TrackKind::Audio), ("pcm_tracks", TrackKind::Pcm)]
			.into_iter()
			.flat_map(|(id, kind)| matches.indices_of(id).into_iter().flatten().map(move |index| (index, kind)))
			.collect_vec();
		tracks.sort_by_key(|(index, _)| *index);
		Self { matches, tracks }
	}

	/// gives each value of the flag to the last track flag before it. `kinds` are the tracks the flag works on
	fn place<T: Clone>(&self, id: &str, flag: &str, values: &[T], kinds: &[TrackKind]) -> anyhow::Result<Vec<Option<T>>> {
		let mut placed = vec![None; self.tracks.len()];
		for (index, value) in self.matches.indices_of(id).into_iter().flatten().zip(values) {
			let track = self.tracks
				.iter()
				.rposition(|(start, _)| *start < index)
				.ok_or_else(|| anyhow::anyhow!("{flag} must come after {}", kinds.iter().map(|kind| kind.flag()).join(" or ")))?;
			let kind = self.tracks[track].1;
			anyhow::ensure!(kinds.contains(&kind), "{flag} does not work on a {} track", kind.flag());
			anyhow::ensure!(placed[track].is_none(), "{flag} is set two times for the same track");
			placed[track] = Some(value.clone());
		}
		Ok(placed)
	}
}

/// the tracks from the track flags
#[derive(Debug)]
struct Tracks {
	video: Vec<VideoSpec>,
	audio: Vec<AudioSpec>,
	pcm: Vec<PcmSpec>,
}

impl Tracks {
	/// gives each track flag to its track. the matches hold the position of each flag on the command line
	fn new(args: &TrackArgs, matches: &ArgMatches) -> anyhow::Result<Self> {
		use TrackKind::*;
		let flags = TrackFlags::new(matches);
		let mut names = flags.place("names", "--name", &args.names, &[Video, Audio, Pcm])?;

		let mut sizes = flags.place("sizes", "--size", &args.sizes, &[Video])?;
		let mut ats = flags.place("ats", "--at", &args.ats, &[Video])?;
		let mut matrices = flags.place("matrices", "--matrix", &args.matrices, &[Video])?;
		let mut gaps = flags.place("gaps", "--gap", &args.gaps, &[Video])?;
		let mut screens = flags.place("screens", "--screen", &args.screens, &[Video])?;
		let mut frame_rates = flags.place("frame_rates", "--frame-rate", &args.frame_rates, &[Video])?;
		let mut filters = flags.place("filters", "--filter", &args.filters, &[Video])?;
		let mut brailles = flags.place("brailles", "--braille", &args.brailles, &[Video])?;
		let mut diffs = flags.place("diffs", "--diff", &args.diffs, &[Video])?;
		let mut budgets = flags.place("budgets", "--budget", &args.budgets, &[Video])?;
		let mut losses = flags.place("losses", "--loss", &args.losses, &[Video])?;
		let mut loss_steps = flags.place("loss_steps", "--loss-step", &args.loss_steps, &[Video])?;

		let mut voices = flags.place("voices", "--voices", &args.voices, &[Audio])?;
		let mut analysis_rates = flags.place("analysis_rates", "--analysis-rate", &args.analysis_rates, &[Audio])?;
		let mut fft_window_sizes = flags.place("fft_window_sizes", "--fft-window-size", &args.fft_window_sizes, &[Audio])?;
		let mut hop_lengths = flags.place("hop_lengths", "--hop-length", &args.hop_lengths, &[Audio])?;
		let mut guards = flags.place("guards", "--guard", &args.guards, &[Audio])?;

		let mut pcm_rates = flags.place("pcm_rates", "--pcm-rate", &args.pcm_rates, &[Pcm])?;
		let mut pcm_aligns = flags.place("pcm_aligns", "--pcm-align", &args.pcm_aligns, &[Pcm])?;

		let mut tracks = Self { video: Vec::new(), audio: Vec::new(), pcm: Vec::new() };
		for (track, (_, kind)) in flags.tracks.iter().enumerate() {
			let name = names[track].take();
			match kind {
				Video => tracks.video.push(VideoSpec {
					name,
					size: sizes[track].take(),
					at: ats[track].take(),
					matrix: matrices[track].take(),
					gap: gaps[track].take(),
					screen: screens[track].take(),
					frame_rate: frame_rates[track].take(),
					filter: filters[track].take(),
					braille: brailles[track].take(),
					diff: diffs[track].take(),
					budget: budgets[track].take(),
					loss: losses[track].take(),
					loss_step: loss_steps[track].take(),
				}),
				Audio => tracks.audio.push(AudioSpec {
					name,
					voices: voices[track].take(),
					analysis_rate: analysis_rates[track].take(),
					fft_window_size: fft_window_sizes[track].take(),
					hop_length: hop_lengths[track].take(),
					guard: guards[track].take(),
				}),
				Pcm => tracks.pcm.push(PcmSpec {
					name,
					rate: pcm_rates[track].take(),
					align: pcm_aligns[track].take(),
				}),
			}
		}
		Ok(tracks)
	}
}

impl Args for Tracks {
	fn augment_args(cmd: clap::Command) -> clap::Command {
		TrackArgs::augment_args(cmd)
	}

	fn augment_args_for_update(cmd: clap::Command) -> clap::Command {
		TrackArgs::augment_args_for_update(cmd)
	}
}

impl FromArgMatches for Tracks {
	fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
		let args = TrackArgs::from_arg_matches(matches)?;
		Self::new(&args, matches).map_err(|e| clap::Error::raw(clap::error::ErrorKind::ArgumentConflict, format!("{e}\n")))
	}

	fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
		*self = Self::from_arg_matches(matches)?;
		Ok(())
	}
}

fn value_name(value: &impl ValueEnum) -> String {
	value.to_possible_value().map_or_else(String::new, |value| value.get_name().to_string())
}

/// the settings the file was encoded with, as "key: value" lines. nested settings are indented
fn describe_settings(args: &Cli, video_config: Option<&VideoConfig>, audio_configs: &[AudioConfig]) -> String {
	let time = |time: Option<Duration>, fallback: &str| time.map_or_else(|| fallback.to_string(), |time| humantime::Duration::from(time).to_string());
	let mut lines = vec![
		format!("encoder: {} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
		format!("encoded: {}", humantime::format_rfc3339_seconds(std::time::SystemTime::now())),
		format!("input: {}", args.in_path.file_name().map_or_else(|| args.in_path.display().to_string(), |name| name.to_string_lossy().to_string())),
		format!("range: {} to {}", time(args.begin, "start"), time(args.end, "end")),
		format!("tier: {}", value_name(&args.tier)),
	];

	if let Some(video_config) = video_config {
		lines.push(format!("video: {} tracks on a {} canvas", video_config.track_descs_data.len(), video_config.container_size));
		lines.push(format!("  fill color: {}", CliSrgb8(video_config.fill_color)));
		for desc in &video_config.track_descs_data {
			lines.push(format!("  track: {}", desc.name));
			lines.push(format!("    size: {}", desc.size));
			if let Some(area) = desc.source_area {
				lines.push(format!("    at: {}", Offset(area.pos)));
			}
			lines.push(format!("    frame rate: {}", desc.frame_rate.map_or_else(|| "source".to_string(), |rate| rate.inverse().to_string())));
			lines.push(format!("    filter: {}", desc.filter.as_ref().map_or_else(|| "none".to_string(), value_name)));
			lines.push(format!("    braille: {}", value_name(&desc.braille_strategy)));
			lines.push(format!("    color diff: {}", value_name(&desc.color_diff_strategy)));
			lines.push(format!("    budget: {}", desc.budget.as_ref().map_or_else(|| "none".to_string(), value_name)));
			lines.push(format!("    loss: {}% (step {}%)", desc.acceptable_loss.into_flt::<f64>() * 100.0, desc.loss_step.into_flt::<f64>() * 100.0));
		}
	}

	for audio in audio_configs {
		lines.push(format!("audio: {}", audio.name));
		lines.push(format!("  mode: {}", value_name(&audio.mode)));
		match audio.mode {
			AudioMode::Voices => {
				lines.push(format!("  voices: {}", audio.num_voices));
				lines.push(format!("  analysis rate: {}Hz", audio.analysis_rate));
				lines.push(format!("  fft window: {}", audio.fft_window_size));
				lines.push(format!("  hop length: {}", audio.hop_length));
				lines.push(format!("  guard: {}", audio.guard));
			},
			AudioMode::Pcm => {
				lines.push(format!("  sample rate: {}Hz (tape speed {:.2})", audio.pcm_rate, dfpwm::tape_speed(audio.pcm_rate)));
				lines.push(format!("  stream align: {} bytes", audio.pcm_align));
			},
		}
	}
	lines.join("\n")
}

/// all video tracks cut their area from one canvas. tracks with `at` (and matrix cells) are placed on it,
/// and the canvas is just large enough to hold them. tracks without `at` show the whole canvas instead
fn build_video_config(fill_color: Srgb<u8>, specs: &[VideoSpec], machine: &Machine, in_path: &Path) -> anyhow::Result<Option<VideoConfig>> {
	if specs.is_empty() {
		return Ok(None);
	}

	let track_desc = |spec: &VideoSpec, name: String, size: Size<usize>, source_area: Option<Rect<usize>>| -> anyhow::Result<VideoDescData> {
		let budget = spec.budget;
		let acceptable_loss = spec.loss;
		let loss_step = spec.loss_step;
		anyhow::ensure!(acceptable_loss.is_none() || budget.is_some(), "video track '{}' has a loss, but no budget", name);
		anyhow::ensure!(loss_step.is_none() || acceptable_loss.is_some(), "video track '{}' has a loss step, but no loss", name);
		Ok(VideoDescData {
			size: size.try_cast().ok_or_else(|| anyhow::anyhow!("video track '{}' is {}, which is too large", name, size))?,
			name,
			frame_rate: spec.frame_rate.map(|x| x.inverse()),
			source_area,
			filter: spec.filter,
			braille_strategy: spec.braille.unwrap_or(BrailleStrategy::CentroidCohesion),
			color_diff_strategy: spec.diff.unwrap_or(ColorDiffStrategy::Fast),
			budget,
			acceptable_loss: acceptable_loss.unwrap_or(Frac::ZERO),
			loss_step: loss_step.unwrap_or(Frac::new(1, 100)),
		})
	};

	let mut source_size = None;
	let mut placed = Vec::new();
	let mut unplaced = Vec::new();
	for (index, spec) in specs.iter().enumerate() {
		let track_size = spec.size;
		if let Some(matrix_size) = spec.matrix {
			anyhow::ensure!(spec.gap.is_none() || spec.screen.is_none(), "a matrix takes gap or screen, not both");
			let track_size = track_size.unwrap_or_else(|| {
				let size = machine.compute_max_resolution(Frac::ONE / video::braille::SIZE.ratio());
				eprintln!("auto-size: {} (largest possible gpu pixel count & width for ratio)", size);
				size
			});
			let track_input_size = track_size * video::braille::SIZE;
			let gap_size = spec.gap
				.or_else(|| spec.screen
					.map(|matrix_screen_size| {
						let gap = compute_gap_size(track_input_size, matrix_screen_size);
						eprintln!("auto-gap: {}", gap);
						gap
					}))
				.unwrap_or(Size::ZERO);
			let origin = spec.at.map_or(Point::ZERO, |at| at.0);
			for cell in (0..matrix_size.h).flat_map(|y| (0..matrix_size.w).map(move |x| Point::new(x, y))) {
				let name = match &spec.name {
					Some(name) => format!("{}:{},{}", name, cell.x, cell.y),
					None => format!("{},{}", cell.x, cell.y),
				};
				placed.push(track_desc(spec, name, track_size, Some(Rect {
					pos: origin + cell * (track_input_size + gap_size),
					size: track_input_size,
				}))?);
			}
		} else {
			anyhow::ensure!(spec.gap.is_none() && spec.screen.is_none(), "gap and screen only work with matrix");
			let track_size = track_size.unwrap_or_else(|| {
				let source_size = *source_size.get_or_insert_with(|| read_video_size(in_path));
				let max_screen_size = machine.compute_max_resolution(source_size.ratio() / video::braille::SIZE.ratio());
				eprintln!("auto-size: {} (largest possible gpu pixel count & width for ratio)", max_screen_size);
				max_screen_size
			});
			let name = default_name(spec.name.clone(), index, specs.len());
			match spec.at {
				Some(at) => placed.push(track_desc(spec, name, track_size, Some(Rect {
					pos: at.0,
					size: track_size * video::braille::SIZE,
				}))?),
				None => unplaced.push(track_desc(spec, name, track_size, None)?),
			}
		}
	}

	if let Some(name) = placed.iter().chain(unplaced.iter()).map(|desc| &desc.name).duplicates().next() {
		anyhow::bail!("there are two video tracks named '{}'", name);
	}

	let container_size = match (placed.is_empty(), unplaced.as_slice()) {
		(true, [first, rest @ ..]) => {
			if let Some(other) = rest.iter().find(|desc| desc.size != first.size) {
				anyhow::bail!("video tracks without `at` show the whole canvas, so they need the same size, but '{}' is {} and '{}' is {}", first.name, first.size, other.name, other.size);
			}
			first.size.cast() * video::braille::SIZE
		},
		(false, []) => placed
			.iter()
			.filter_map(|desc| desc.source_area)
			.fold(Size::ZERO, |size: Size<usize>, area| Size::new(
				size.w.max(area.pos.x + area.size.w),
				size.h.max(area.pos.y + area.size.h),
			)),
		_ => anyhow::bail!("video tracks without `at` show the whole canvas, so they can not share it with tracks that have `at` or `matrix`"),
	};

	Ok(Some(VideoConfig {
		track_descs_data: placed.into_iter().chain(unplaced).collect(),
		container_size,
		fill_color,
	}))
}

fn read_video_size(in_path: &Path) -> Size<usize> {
	let ictx = ffmpeg_next::format::input(in_path).expect("failed to create decoder");
	let stream = ictx.streams().best(ffmpeg_next::media::Type::Video).unwrap();
	let codec_ctx = ffmpeg_next::codec::Context::from_parameters(stream.parameters()).expect("failed to create codec context");
	let decoder = codec_ctx.decoder().video().unwrap();
	Size::new(decoder.width() as usize, decoder.height() as usize)
}

pub fn compute_gap_size(track_size: Size<usize>, matrix_screen_size: Size<usize>) -> Size<usize> {
	const FIXED_POINT: usize = 2;
	const PIXEL_GAP: usize = 9; //it's '(2 + 0.25) * 2' but we use x2 fixed point to remove the floats
	const MINECRAFT_PIXELS: usize = 16;
	//https://www.desmos.com/calculator/balbctweiy
	(track_size * PIXEL_GAP) / (matrix_screen_size * (MINECRAFT_PIXELS * FIXED_POINT) - PIXEL_GAP)
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum VideoFilter {
	Monochrome,
	Grayscale,
	Vga,
	Hsv,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum Budget {
	Direct,
	Buffered,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum BrailleStrategy {
	CentroidCohesion,
	PolarPair,
	PalettePair,
	PaletteDither,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum ColorDiffStrategy {
	#[value(name = "fast")] Fast,
	#[value(name = "ciede")] ImprovedCiede2000,
}

/// the voices tracks, then the pcm tracks, each in the order of their flags
fn build_audio_configs(args: &AudioOpts, audio_specs: &[AudioSpec], pcm_specs: &[PcmSpec]) -> anyhow::Result<Vec<AudioConfig>> {
	let config = |name: String, mode: AudioMode| AudioConfig {
		name,
		mode,
		pcm_rate: args.pcm_rate,
		pcm_align: args.pcm_align,
		analysis_rate: args.analysis_rate,
		fft_window_size: args.fft_window_size,
		hop_length: args.hop_length.unwrap_or(args.fft_window_size / 2),
		guard: args.guard,
		normalize: true,
		num_voices: args.num_voices,
	};

	let mut configs = Vec::new();
	for (index, spec) in audio_specs.iter().enumerate() {
		let mut config = config(default_name(spec.name.clone(), index, audio_specs.len()), AudioMode::Voices);
		if let Some(num_voices) = spec.voices {
			anyhow::ensure!(num_voices >= 1, "audio track '{}' needs at least 1 voice", config.name);
			config.num_voices = num_voices;
		}
		config.analysis_rate = spec.analysis_rate.unwrap_or(config.analysis_rate);
		if let Some(fft_window_size) = spec.fft_window_size {
			config.fft_window_size = parse_power_of_two(&fft_window_size.to_string()).map_err(anyhow::Error::msg)?;
			config.hop_length = args.hop_length.unwrap_or(fft_window_size / 2);
		}
		config.hop_length = spec.hop_length.unwrap_or(config.hop_length);
		config.guard = spec.guard.unwrap_or(config.guard);
		configs.push(config);
	}
	for (index, spec) in pcm_specs.iter().enumerate() {
		let mut config = config(default_name(spec.name.clone(), index, pcm_specs.len()), AudioMode::Pcm);
		if let Some(pcm_rate) = spec.rate {
			config.pcm_rate = parse_pcm_rate(&pcm_rate.to_string()).map_err(anyhow::Error::msg)?;
		}
		if let Some(pcm_align) = spec.align {
			anyhow::ensure!(pcm_align >= 1, "pcm track '{}' needs an align of at least 1", config.name);
			config.pcm_align = pcm_align;
		}
		configs.push(config);
	}

	if let Some((mode, name)) = configs.iter().map(|config| (config.mode, &config.name)).duplicates().next() {
		anyhow::bail!("there are two {} tracks named '{}'", value_name(&mode), name);
	}
	Ok(configs)
}
const TRACKS_HELP: &str = "\
Tracks:
  Each --video, --audio or --pcm adds a track.
  The track flags after it set only that track.
  A track flag must come after a track flag of its kind.

Example:
  encode -i in.mp4 --video --name left --at 0+0 -B palette-pair --video --name right --at 320+0 -B palette-pair --pcm";

#[derive(Parser, Debug)]
#[command(after_help = TRACKS_HELP)]
pub struct Cli {
	#[arg(
		short = 'i',
		long = "in",
		help = "Path to image or video to encode",
		value_hint = ValueHint::FilePath,
	)]
	pub in_path: PathBuf,

	#[arg(
		short = 'o',
		long = "out",
		help = "Path to the output SZT file or folder, saves next to the input with .szt extension if omitted",
		value_hint = ValueHint::FilePath,
	)]
	pub out_path: Option<PathBuf>,


	#[arg(
		short = 's',
		long = "start",
		help = "Time to start from, starts at input start if omitted",
		value_parser = humantime::parse_duration,
	)]
	pub begin: Option<Duration>,

	#[arg(
		short = 'e',
		long = "end",
		help = "Time to stop at (inclusive), stops at input end if omitted",
		value_parser = humantime::parse_duration,
	)]
	pub end: Option<Duration>,

	#[arg(
		value_enum,
		short = 't',
		long = "tier",
		help = "What machine tier to optimize for",
		default_value_t = Tier::T3,
	)]
	tier: Tier,

	#[arg(
		long = "tracks-config",
		help = "Path to a json file with more tracks: {\"video\": [...], \"audio\": [...], \"pcm\": [...]}. each track is an object, its keys are the track flags without the dashes (\"rate\" and \"align\" for --pcm-rate and --pcm-align)",
		value_hint = ValueHint::FilePath,
	)]
	tracks_config: Option<PathBuf>,

	#[arg(
		long = "fill-color",
		help = "color of the padding around the video, as RRGGBB hex",
		default_value_t = Srgb::<u8>::from_u32::<Argb>(0x000000).into(),
	)]
	fill_color: CliSrgb8,

	#[command(flatten)]
	tracks: Tracks,
}

/// the track flags as clap gives them, before they are given to their tracks
#[derive(Args, Debug)]
struct TrackArgs {
	#[arg(
		long = "video",
		num_args = 0,
		default_missing_value = "true",
		action = ArgAction::Append,
		help_heading = "Tracks",
		help = "Add a video track. the video flags after it set this track",
	)]
	video_tracks: Vec<bool>,

	#[arg(
		long = "audio",
		num_args = 0,
		default_missing_value = "true",
		action = ArgAction::Append,
		help_heading = "Tracks",
		help = "Add a sound card voices track. the audio flags after it set this track",
	)]
	audio_tracks: Vec<bool>,

	#[arg(
		long = "pcm",
		num_args = 0,
		default_missing_value = "true",
		action = ArgAction::Append,
		help_heading = "Tracks",
		help = "Add a DFPWM track for computronics tape drives, play it with play_szt --pcm-tape. the pcm flags after it set this track",
	)]
	pcm_tracks: Vec<bool>,

	#[arg(
		long = "name",
		value_name = "NAME",
		help_heading = "Tracks",
		help = "Name of the track. a lone track of its kind is \"main\" if omitted, else its index",
	)]
	names: Vec<String>,

	#[arg(
		long = "size",
		value_name = "WxH",
		help_heading = "Video track",
		help = "Size of the track in characters, fits the GPU limits if omitted",
	)]
	sizes: Vec<Size<usize>>,

	#[arg(
		long = "at",
		value_name = "X+Y",
		help_heading = "Video track",
		help = "Where the track is on the canvas, in sub-pixels. without it the track shows the whole canvas",
	)]
	ats: Vec<Offset>,

	#[arg(
		long = "matrix",
		value_name = "WxH",
		help_heading = "Video track",
		help = "Make a grid of tracks, named \"x,y\" (or \"name:x,y\")",
	)]
	matrices: Vec<Size<usize>>,

	#[arg(
		long = "gap",
		value_name = "WxH",
		help_heading = "Video track",
		help = "Gap between the tracks of a matrix, in sub-pixels",
	)]
	gaps: Vec<Size<usize>>,

	#[arg(
		long = "screen",
		value_name = "WxH",
		help_heading = "Video track",
		help = "Screen size of each matrix track in blocks, computes the gap from the screen borders",
	)]
	screens: Vec<Size<usize>>,

	#[arg(
		short = 'f',
		long = "frame-rate",
		value_name = "FPS",
		help_heading = "Video track",
		help = "Output framerate, copies input framerate if omitted",
	)]
	frame_rates: Vec<Frac<u16>>,

	#[arg(
		value_enum,
		long = "filter",
		value_name = "FILTER",
		help_heading = "Video track",
		help = "What to pass the pixels through before encoding",
	)]
	filters: Vec<VideoFilter>,

	#[arg(
		value_enum,
		short = 'B',
		long = "braille",
		value_name = "STRATEGY",
		help_heading = "Video track",
		help = "Brailling strategy [default: centroid-cohesion]",
	)]
	brailles: Vec<BrailleStrategy>,

	#[arg(
		value_enum,
		short = 'D',
		long = "diff",
		value_name = "STRATEGY",
		help_heading = "Video track",
		help = "Color comparison strategy [default: fast]",
	)]
	diffs: Vec<ColorDiffStrategy>,

	#[arg(
		value_enum,
		short = 'b',
		long = "budget",
		value_name = "BUDGET",
		help_heading = "Video track",
		help = "When enabled, the video will lower in framerate if the frame complexity becomes higher than the GPU render budget",
	)]
	budgets: Vec<Budget>,

	#[arg(
		short = 'l',
		long = "loss",
		value_name = "LOSS",
		help_heading = "Video track",
		help = "Highest loss a frame may have when it's over budget, needs a budget [default: 0]",
	)]
	losses: Vec<Frac<u32>>,

	#[arg(
		long = "loss-step",
		value_name = "STEP",
		help_heading = "Video track",
		help = "How much to raise the loss each time a frame is over budget, needs a loss [default: 1%]",
	)]
	loss_steps: Vec<Frac<u32>>,

	#[arg(
		short = 'v',
		long = "voices",
		value_name = "N",
		help_heading = "Audio track",
		help = "Number of voices (8 channels per sound card) [default: 8]",
		value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..),
	)]
	voices: Vec<usize>,

	#[arg(
		long = "analysis-rate",
		value_name = "HZ",
		help_heading = "Audio track",
		help = "Target analysis sample rate (Hz) [default: 24000]",
	)]
	analysis_rates: Vec<u32>,

	#[arg(
		long = "fft-window-size",
		value_name = "N",
		help_heading = "Audio track",
		help = "FFT window size (power of two) [default: 1024]",
		value_parser = parse_power_of_two,
	)]
	fft_window_sizes: Vec<usize>,

	#[arg(
		long = "hop-length",
		value_name = "N",
		help_heading = "Audio track",
		help = "Hop size in samples (defaults to window/2)",
	)]
	hop_lengths: Vec<usize>,

	#[arg(
		long = "guard",
		value_name = "N",
		help_heading = "Audio track",
		help = "Bins around a selected peak that will be removed [default: 1]",
		allow_negative_numbers = true,
	)]
	guards: Vec<isize>,

	#[arg(
		long = "pcm-rate",
		value_name = "HZ",
		help_heading = "PCM track",
		help = "Sample rate (Hz). the tape drive plays 48000 at speed 1, so this sets the speed (12000-96000, a multiple of 32) [default: 48000]",
		value_parser = parse_pcm_rate,
	)]
	pcm_rates: Vec<u32>,

	#[arg(
		long = "pcm-align",
		value_name = "BYTES",
		help_heading = "PCM track",
		help = "Each pcm track gets its own stream, which starts on a multiple of this many bytes [default: 4096]",
		value_parser = clap::value_parser!(u32).range(1..),
	)]
	pcm_aligns: Vec<u32>,
}

#[derive(Debug, Clone)]
struct CliSrgb8(Srgb<u8>);

#[derive(thiserror::Error, Debug)]
pub enum ParseColorError {
	#[error("Color must be 6 hex digits, {0} supplied")]
	BadCharacterCount(usize),
	#[error("{0}")]
	ParseIntError(#[from] ParseIntError),
}

impl FromStr for CliSrgb8 {
	type Err = ParseColorError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let hex = s.trim();
		if hex.len() != 6 {
			return Err(ParseColorError::BadCharacterCount(hex.len()));
		}
		let r = u8::from_str_radix(&hex[0..2], 16)?;
		let g = u8::from_str_radix(&hex[2..4], 16)?;
		let b = u8::from_str_radix(&hex[4..6], 16)?;
		Ok(CliSrgb8(Srgb::new(r, g, b)))
	}
}

impl Display for CliSrgb8 {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{:02X}{:02X}{:02X}", self.red, self.green, self.blue)
	}
}

impl Deref for CliSrgb8 {
	type Target = Srgb::<u8>;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl From<Srgb<u8>> for CliSrgb8 {
	fn from(value: Srgb<u8>) -> Self {
		Self(value)
	}
}
/// the values the audio and pcm tracks use for the keys their spec leaves out
#[derive(Debug)]
pub struct AudioOpts {
	pub pcm_rate: u32,
	pub pcm_align: u32,
	pub analysis_rate: u32,
	pub fft_window_size: usize,
	pub hop_length: Option<usize>,
	pub guard: isize,
	pub num_voices: usize,
}

impl Default for AudioOpts {
	fn default() -> Self {
		Self {
			pcm_rate: dfpwm::TAPE_SAMPLE_RATE,
			pcm_align: 4096,
			analysis_rate: 24000,
			fft_window_size: 1024,
			hop_length: None,
			guard: 1,
			num_voices: 8,
		}
	}
}

fn parse_pcm_rate(s: &str) -> Result<u32, String> {
	let value = s.parse::<u32>().map_err(|e| e.to_string())?;
	let speed = dfpwm::tape_speed(value);
	if !(dfpwm::TAPE_MIN_SPEED..=dfpwm::TAPE_MAX_SPEED).contains(&speed) {
		return Err(format!("{value}Hz needs tape speed {speed:.2}, but tape drives only play at {}-{}", dfpwm::TAPE_MIN_SPEED, dfpwm::TAPE_MAX_SPEED));
	}
	if value % dfpwm::TAPE_SAMPLE_RATE_STEP != 0 {
		return Err(format!("{value}Hz is not a multiple of {}, which is the step the tape drive speed has", dfpwm::TAPE_SAMPLE_RATE_STEP));
	}
	Ok(value)
}

fn parse_power_of_two(s: &str) -> Result<usize, String> {
	let value = s.parse::<usize>().map_err(|e| e.to_string())?;
	if !value.is_power_of_two() {
		return Err(format!("{value} is not a power of two"));
	}
	Ok(value)
}
