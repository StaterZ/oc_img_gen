import std.core;
import std.sys;
import std.array;

#pragma endian big

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
	return std::format("({},{})", value.x, value.y);
};

struct Color {
	u8 color;
};

enum CommandKind: u8 {
	Text = 0,
	Braille = 1,
};

bitfield CommandFlags {
	bool has_background : 1;
	bool has_foreground : 1;
	unsigned len : 6 [[transform("len_trs")]];
};
fn len_trs(u8 len) {
	return len + 1;
};


struct Command<auto kind> {
    std::print("cmd start: 0x{:x}", $);
	CommandFlags flags;
	
	if (flags.has_background) Color background;
	if (flags.has_foreground) Color foreground;
	
	Point pos;
	
	match (kind) {
		(CommandKind::Text): std::Array<char, flags.len> text;
		(CommandKind::Braille): std::Array<u8, flags.len> braille;
	}
};

struct Header {
	char magic[4];
	u16 version;
	Size size;
	u16 frame_rate;
	u32 num_frames;
	
	std::assert(magic == "sztb", "bad magic");
	std::assert(version == 2, "bad version");
};

struct Frame<auto commands_len> {
	CommandKind command_kind;
	match (command_kind) {
		(CommandKind::Text): std::ByteSizedArray<Command<CommandKind::Text>, commands_len - sizeof(command_kind)> commands;
		(CommandKind::Braille): std::ByteSizedArray<Command<CommandKind::Braille>, commands_len - sizeof(command_kind)> commands;
	}
};

struct File {
	Header header;
	u32 frames_start = $;
	std::Array<u32, header.num_frames> frame_sizes;
	u64 start = $;
    std::print("geh: {}", start);
	Frame<frame_sizes.data[std::core::array_index()]> frames[header.num_frames];
};

File file @ 0x00;
