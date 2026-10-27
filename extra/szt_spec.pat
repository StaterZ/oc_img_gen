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
//one DFPWM block after the packets, for computronics tape drives
struct PcmDesc {
	u32 sample_rate;
	u32 offset; //from the start of the file
	u32 num_bytes;
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
	std::string::SizedString<u16> settings; //"key: value" lines
	
	std::assert(magic == "sztb", "bad magic");
	std::assert(version == 7, "bad version");
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
	match (parent.track_descs[track_id].kind) {
		(DescKind::Video): Frame<parent.track_descs[track_id].desc> content;
		(DescKind::Audio): Sample<parent.track_descs[track_id].desc> content;
	}
};

fn calculate_total_packets(ref auto track_descs, u32 num_tracks) {
	u32 total = 0;
	for (u32 i = 0, i < num_tracks, i += 1) {
		total += track_descs[i].num_packets;
	}
	return total;
};

struct File {
	Header header;
	Desc track_descs[header.num_tracks];
	u32 num_packets = calculate_total_packets(track_descs, header.num_tracks);
	
	u64 start = $;
	std::print("start: {}", start);
	Packet packets[num_packets];
	u8 pcm_data[std::mem::size() - $];
};

File file @ 0x00;
