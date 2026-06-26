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
	unsigned len : 6 [[transform("len_trs")]];
	bool has_foreground : 1;
	bool has_background : 1;
};
fn len_trs(u8 len) {
	return len + 1;
};


struct Command<auto kind> {
	//std::print("cmd start: 0x{:x}", $);
	CommandFlags flags;
	
	if (flags.has_background) Color background;
	if (flags.has_foreground) Color foreground;
	
	Point pos;
	
	match (kind) {
		(CommandKind::Text): std::Array<char, flags.len> text;
		(CommandKind::Braille): std::Array<u8, flags.len> braille;
	}
};

struct VideoDesc {
	Size size;
};
struct AudioDesc {
	u8 num_voices;
};
enum DescKind: u8 {
	Video = 0x00,
	Audio = 0x01,
};
struct Desc {
	u32 num_packets;
	Frac rate;
	std::string::SizedString<u8> name;
	DescKind kind;
	match (kind) {
		(DescKind::Video): VideoDesc desc;
		(DescKind::Audio): AudioDesc desc;
	}
};

struct Header {
	char magic[4];
	u16 version;
	u8 num_streams;
	
	std::assert(magic == "sztb", "bad magic");
	std::assert(version == 5, "bad version");
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
	u8 stream_id;
	match (parent.stream_descs[stream_id].kind) {
		(DescKind::Video): Frame<parent.stream_descs[stream_id].desc> content;
		(DescKind::Audio): Sample<parent.stream_descs[stream_id].desc> content;
	}
};

fn calculate_total_packets(ref auto stream_descs, u32 num_streams) {
	u32 total = 0;
	for (u32 i = 0, i < num_streams, i += 1) {
		total += stream_descs[i].num_packets;
	}
	return total;
};

struct File {
	Header header;
	Desc stream_descs[header.num_streams];
	u32 num_packets = calculate_total_packets(stream_descs, header.num_streams);
	
	u64 start = $;
	std::print("start: {}", start);
	Packet packets[num_packets];
};

File file @ 0x00;
