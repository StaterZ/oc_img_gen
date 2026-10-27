import std.core;
import std.sys;
import std.array;
import std.string;

#pragma endian little
#pragma pattern_limit 100000000

struct Point {
	u8 x;
	u8 y;
} [[format("point_fmt")]];
fn point_fmt(Point value) {
	return std::format("({},{})", value.x, value.y);
};

struct Size {
	u8 x;
	u8 y;
} [[format("size_fmt")]];
fn size_fmt(Size value) {
	return std::format("({}x{})", value.x, value.y);
};

struct Frac {
	u16 numerator;
	u16 denominator;
} [[format("frac_fmt")]];
fn frac_fmt(Frac value) {
	return std::format("({}/{})", value.numerator, value.denominator);
};

struct Color {
	u8 color;
};

enum CommandKind: u8 {
	Text = 0x00,
	Braille = 0x01,
};

bitfield CommandFlags {
	unsigned len : 5 [[transform("len_trs")]];
	bool is_rle : 1;
	bool has_foreground : 1;
	bool has_background : 1;
};
fn len_trs(u8 len) {
	return len + 1;
};

struct RleRun {
	u8 val;
	u8 len;
};

struct Command<auto kind> {
	//std::print("cmd start: 0x{:x}", $);
	CommandFlags flags;
	
	if (flags.has_background) Color background;
	if (flags.has_foreground) Color foreground;
	
	Point pos;
	
	match (kind) {
		(CommandKind::Text): std::Array<char, flags.len> text;
		(CommandKind::Braille): match (flags.is_rle) {
			(false): std::Array<u8, flags.len> braille;
			(true): std::Array<RleRun, flags.len> braille;
		}
	}
};

struct VideoDesc {
	Size size;
};
struct AudioDesc {
	u8 num_voices;
};
//one DFPWM block, alone on its stream, so computronics tape drives can play the stream as is
struct PcmDesc {
	u32 sample_rate;
	u32 checksum; //crc32 of the block
};
enum DescKind: u8 {
	Video = 0x00,
	Audio = 0x01,
	Pcm = 0x02,
};
struct Desc {
	u32 num_packets;
	Frac rate;
	std::string::SizedString<u8> name;
	u8 stream_id; //the stream that holds the packets of this track
	DescKind kind;
	match (kind) {
		(DescKind::Video): VideoDesc desc;
		(DescKind::Audio): AudioDesc desc;
		(DescKind::Pcm): PcmDesc desc;
	}
};

struct Header {
	char magic[4];
	u16 version;
	u8 num_tracks;
	u8 num_streams;
	std::string::SizedString<u16> settings; //"key: value" lines
	
	std::assert(magic == "sztb", "bad magic");
	std::assert(version == 8, "bad version");
};

struct Frame<auto desc> {
	u16 commands_len;
	CommandKind command_kind;
	match (command_kind) {
		(CommandKind::Text): std::ByteSizedArray<Command<CommandKind::Text>, commands_len> commands;
		(CommandKind::Braille): std::ByteSizedArray<Command<CommandKind::Braille>, commands_len> commands;
	}
};

struct Array2D<T, auto N_INNER, auto N_OUTER> {
	std::Array<std::Array<T, N_INNER> , N_OUTER> data;
};

struct VoiceState {
	u8 volume;
	u16 frequency;
};
struct Sample<auto desc> {
	VoiceState voices[desc.num_voices];
};

struct Packet {
	u8 track_id;
	match (file.track_descs[track_id].kind) {
		(DescKind::Video): Frame<file.track_descs[track_id].desc> content;
		(DescKind::Audio): Sample<file.track_descs[track_id].desc> content;
	}
};

//where a stream is. a stream is the interleaved packets of its tracks, or the DFPWM block of one pcm track.
//the encoder can align where a stream starts, so there can be padding before it
struct StreamDesc {
	u32 offset; //from the start of the file
	u32 num_bytes;
};

fn stream_num_packets(u32 stream_id) {
	u32 total = 0;
	for (u32 i = 0, i < file.header.num_tracks, i += 1) {
		if (file.track_descs[i].stream_id == stream_id) {
			total += file.track_descs[i].num_packets;
		}
	}
	return total;
};

fn stream_is_pcm(u32 stream_id) {
	for (u32 i = 0, i < file.header.num_tracks, i += 1) {
		if (file.track_descs[i].stream_id == stream_id) {
			return file.track_descs[i].kind == DescKind::Pcm;
		}
	}
	return false;
};

struct Stream {
	u32 stream_id = std::core::array_index();
	u32 offset = file.stream_descs[stream_id].offset;
	u32 num_bytes = file.stream_descs[stream_id].num_bytes;
	if (stream_is_pcm(stream_id)) {
		u8 pcm[num_bytes] @ offset;
	} else {
		Packet packets[stream_num_packets(stream_id)] @ offset;
	}
};

struct File {
	Header header;
	Desc track_descs[header.num_tracks];
	StreamDesc stream_descs[header.num_streams];
	Stream streams[header.num_streams];
};

File file @ 0x00;
