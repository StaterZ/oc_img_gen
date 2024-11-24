#include "std/sys.pat"

struct Slice<TLen, TData> {
	TLen len;
	TData data[len];
};

struct ByteSlice<TLen, TData> {
	TLen len;
	u64 s = $;
	TData data[while($ - s < len)];
};

struct Point {
	u8 x;
	u8 y;
};

struct Size {
	u8 x;
	u8 y;
};

struct Color {
	u8 color;
};

enum CommandKind : u8 {
	Set = 0x00 ... 0x7f,
	SetBackground = 0x80,
	SetForeground = 0x81,
	SetResolution = 0x82,
};

struct SetCommand<auto len> {
	Point pos;
	char chars[len];
};
struct SetBackgroundCommand {
	Color color;
};
struct SetForegroundCommand {
	Color color;
};
struct SetResolutionCommand {
	Size size;
};

struct SztCommand {
	CommandKind kind;
	match (kind) {
		(CommandKind::SetBackground): SetBackgroundCommand;
		(CommandKind::SetForeground): SetForegroundCommand;
		(CommandKind::SetResolution): SetResolutionCommand;
		(_): if (kind >= 0x00 && kind <= 0x7f) {
			SetCommand<u8(kind)>;
		}
	}
};

struct SztHeader {
	char magic[4];
	be u16 version;
	Size size;
	be u16 frame_rate;
};

struct SztFrame {
	ByteSlice<be u64, SztCommand> commands;
	//Slice<be u64, u8> commands_blob;
};

struct SztFile {
	SztHeader header;
	Slice<be u64, SztFrame> frames;
};

SztFile file @ 0x00;

std::assert(file.header.magic == "sztb", "bad magic");
std::assert(file.header.version == 1, "bad version");
