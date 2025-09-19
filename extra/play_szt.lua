local os = require("os")
local io = require("io")
local shell = require("shell")
local event = require("event")
local unicode = require("unicode")
local component = require("component")
local computer = require("computer")
local term = require("term")
local serialization = require("serialization")

local version = "1.2"

local linear_stream = {}
do
	local mt = {
		__index = linear_stream,
		__metatable = "LinearStream"
	}

	function linear_stream.open(path, format)
		assert(format == "rb")
		local stream, reason = io.open(path, format)
		if not stream then return nil, reason end

		return setmetatable({
			stream = stream,
			buffer = "",
			bufferHead = 0,
			bufferSize = math.max(512, math.min(8 * 1024, computer.freeMemory() / 8))
		}, mt)
	end

	function linear_stream:close()
		self.buffer = nil
		self.stream:close()
	end

	function linear_stream:seek(whence, offset) --shamelessly stolen from OpenOS buffer code
		whence = whence or "cur"
		assert(whence == "set" or whence == "cur" or whence == "end",
			"bad argument #1 (set, cur or end expected, got " .. whence .. ")")
		offset = offset or 0
		checkArg(2, offset, "number")
		assert(math.floor(offset) == offset, "bad argument #2 (not an integer)")

		if whence == "cur" then
			offset = offset - (#self.buffer - self.bufferHead)
		end
		local result, reason = self.stream:seek(whence, offset)
		if not result then return nil, reason end

		self.buffer = ""
		self.bufferHead = 0
		return result
	end

	function linear_stream:read(n)
		if n <= #self.buffer - self.bufferHead then
			local data = self.buffer:sub(1 + self.bufferHead, self.bufferHead + n)
			--print(#self.buffer, n, "base", self.bufferHead, ("'%s'"):format(data))
			self.bufferHead = self.bufferHead + n
			return data
		end

		local data = self.buffer:sub(1 + self.bufferHead)
		--print(#self.buffer, n, "stitch begin", self.bufferHead, ("'%s'"):format(data))
		--self.bufferHead = #self.buffer --no need to advance head, we'll overwrite it shortly anyways...

		local needed = n - #data
		while true do
			local result, reason = self.stream:read(self.bufferSize)
			if not result then
				if reason then
					return result, reason
				else
					error("read past EOF")
				end
			end
			self.buffer = result

			if needed >= #self.buffer then
				data = data .. self.buffer
				needed = n - #data
				--print(#self.buffer, needed, "stitch spin", ("'%s'"):format(data))
			else
				data = data .. self.buffer:sub(1, needed)
				self.bufferHead = needed
				--print(#self.buffer, needed, "stitch end", self.bufferHead, ("'%s'"):format(data))
				break
			end
		end

		return data
	end

	function linear_stream:size()
		return #self.buffer - self.bufferHead
	end
end

local function assertEq(found, expected, msg)
	assert(found == expected, ("%s: expected '%s', found '%s'"):format(msg, expected, format))
end

local function read_u8(file)
	return file:read(1):byte()
end

local function read_u16(file)
	return
		read_u8(file) * 0x100 +
		read_u8(file)
end

local function read_u32(file)
	return
		read_u16(file) * 0x10000 +
		read_u16(file)
end

local function read_u64(file)
	return
		read_u32(file) * 0x100000000 +
		read_u32(file)
end

local function time_fmt(secs)
	local result = ("%2.1fs"):format(secs % 60)
	if secs >= 60 then
		result = ("%im %s"):format(math.floor(secs / 60), result)
	end
	return result
end

local szt = {
	magic = "sztb",
	version = 3,
}

local args, ops = shell.parse(...)
ops.no_back = ops["no-back"]
ops.batch_check = ops["batch-check"]
ops.color_check = ops["color-check"]

local function inflate(v)
	if v < 16 then
		return v, true
		--return ((v + 1) / 17 * 0xff) * 0x010101, false
	else
		local NUM_REDS, NUM_GREENS, NUM_BLUES = 6, 8, 5
		local i = v - 16

		local i_r = math.floor(i / (NUM_GREENS * NUM_BLUES))
		local i_g = math.floor(i / NUM_BLUES) % NUM_GREENS
		local i_b = i % NUM_BLUES

		local r = math.min(math.floor((i_r * 0x100) / (NUM_REDS - 1)), 0xff)
		local g = math.min(math.floor((i_g * 0x100) / (NUM_GREENS - 1)), 0xff)
		local b = math.min(math.floor((i_b * 0x100) / (NUM_BLUES - 1)), 0xff)

		return r * 0x10000 + g * 0x100 + b, false
		--return r << 16 + g << 8 + b, false
	end
end

local frame_header_size = 1 -- 1 from the command_kind (this is a constant)
local function draw_stream_frame(gpu, file, stream, frame_index)
	local pos_x, pos_y = stream.surface.pos_x, stream.surface.pos_y

	local command_kind = read_u8(file)

	local get_value
	if command_kind == 1 then --check 1 first since it's likely more common
		get_value = function(len)
			value = ""
			for i = 1, len do
				value = value .. unicode.char(0x2800 + read_u8(file))
			end
			return value
		end
	elseif command_kind == 0 then
		get_value = function(len)
			return file:read(len)
		end
	else
		error(("bad frame command_kind '%i'"):format(command_kind))
	end

	if ops.batch_check then
		get_value = function(len)
			file:read(len) --more efficient than seek
			gpu.setBackground(math.random(0xffffff))
			return (" "):rep(len)
		end
	end

	if ops.color_check then
		get_value = function(len, i)
			file:read(len) --more efficient than seek
			gpu.setBackground((i==0 and 0x010101 or i==1 and 0x000100 or 0x010000)*math.max(1, math.ceil(math.ceil(len*8/0x40)/8*0xff)))
			return (" "):rep(len)
		end
	end

	local commands_len = stream.frame_sizes[frame_index + 1] - frame_header_size
	local command_count = 0
	local i = 0

	local len, x, y
	while i < commands_len do
		len = read_u8(file)
		local q = i
		if len >= 0x80 then
			len = len - 0x80

			gpu.setBackground(inflate(read_u8(file)))
			i = i + 1
		end
		if len >= 0x40 then
			len = len - 0x40

			gpu.setForeground(inflate(read_u8(file)))
			i = i + 1
		end

		len = len + 1
		x = read_u8(file)
		y = read_u8(file)
		gpu.set(x + pos_x, y + pos_y, get_value(len, i-q))
		i = i + 3 + len
		command_count = command_count + 1
	end
	return command_count
end

local function read_header(file)
	local magic = file:read(4)
	local version = read_u16(file)
	local frame_rate = read_u16(file)
	local num_frames = read_u32(file)
	local num_streams = read_u8(file)

	local streams = {}
	for i = 1, num_streams do
		local size_x = read_u8(file)
		local size_y = read_u8(file)
		local name = file:read(read_u8(file))

		table.insert(streams, {
			name = name,
			size_x = size_x,
			size_y = size_y,
		})
	end

	return {
		magic = magic,
		version = version,
		frame_rate = frame_rate,
		num_frames = num_frames,
		num_streams = num_streams,
		streams = streams,
	}
end

local function probe_header(file)
	local header = read_header(file)
	print(("magic: %s %s"):format(header.magic, header.magic == szt.magic and "OK" or "ERR"))
	print(("version: %i %s"):format(header.version, header.version == szt.version and "OK" or "OLD"))
	print(("frame rate: %i"):format(header.frame_rate))
	print(("frame count: %i"):format(header.num_frames))

	print(("found %i stream descriptors:"):format(header.num_streams))
	for i, stream in ipairs(header.streams) do
		print(("%4i: '%s' %ix%i"):format(
			i,
			stream.name,
			stream.size_x,
			stream.size_y
		))
	end

	print(("seek tables: (%iframes x %istreams)"):format(header.num_frames, header.num_streams))
	local size_min = math.huge
	local size_sum = 0
	local size_max = 0
	for stream_i = 1, header.num_streams do
		for frame_i = 1, header.num_frames do
			local frame_size = read_u32(file)
			size_min = math.min(size_min, frame_size)
			size_sum = size_sum + frame_size
			size_max = math.max(size_max, frame_size)
		end
	end
	local size_avg = size_sum / (header.num_streams * header.num_frames)

	print(("    min frame bytes: %i"):format(size_min))
	print(("    avg frame bytes: %f"):format(size_avg))
	print(("    max frame bytes: %i"):format(size_max))

	local frames_begin_pos = file:seek()
	print(("headers done at byte: %i"):format(frames_begin_pos))
end

local function render(gpu, file, surfaces)
	local header = read_header(file)
	assertEq(header.magic, szt.magic, "bad magic")
	print("magic: OK")
	assertEq(header.version, szt.version, "bad version")
	print("version: OK")

	local frame_rate = header.frame_rate
	local num_frames = header.num_frames
	local num_streams = header.num_streams

	local main_screen = gpu.getScreen()
	local max_size_x, max_size_y = 0, 0
	local streams = {}
	for i, stream_desc in ipairs(header.streams) do
		local surface = surfaces[stream_desc.name] or error(("missing surface for stream '%s'"):format(stream_desc.name))
		surface.pos_x = surface.is_fullscreen and 1 or surface.pos_x or error("surface has no pos_x")
		surface.pos_y = surface.is_fullscreen and 1 or surface.pos_y or error("surface has no pos_y")

		local stream = {
			size_x = stream_desc.size_x,
			size_y = stream_desc.size_y,
			name = stream_desc.name,
			surface = surface,
			frame_sizes = {},
		}

		max_size_x = math.max(max_size_x, stream.size_x)
		max_size_y = math.max(max_size_y, stream.size_y)

		if stream.surface.is_fullscreen then
			gpu.bind(stream.surface.screen_addr, false)
			gpu.setResolution(stream.size_x, stream.size_y)
		end

		table.insert(streams, stream)
	end
	if gpu.getScreen() ~= main_screen then
		gpu.bind(main_screen, false)
	end

	print(("reading seek tables... (%iframes x %istreams)"):format(num_frames, num_streams))
	local seek_table = {}
	for stream_index, stream in ipairs(streams) do
		for i = 1, num_frames do
			local frame_size = read_u32(file)
			stream.frame_sizes[i] = frame_size
			seek_table[i] = (seek_table[i] or 0) + frame_size
		end
	end
	for i = 2, #seek_table do
		seek_table[i] = seek_table[i] + seek_table[i - 1]
	end

	local frames_begin_pos = file:seek()
	local back
	if not ops.no_back then
		back = gpu.allocateBuffer(max_size_x, max_size_y)
		if back == nil then error("can't allocate back-buffer") end

		gpu.setActiveBuffer(back)
	end

	local function draw()
		local video_begin_time, video_begin_time_up = os.clock(), computer.uptime()
		local frame_index = 0
		while frame_index < num_frames do
			local frame_begin_time, frame_begin_time_up = os.clock(), computer.uptime()

			if ops.seek then
				local current_time = (computer.uptime() - video_begin_time_up)
				frame_index = math.ceil(current_time * frame_rate)
				if frame_index >= num_frames then break end

				file:seek("set", frames_begin_pos + seek_table[frame_index + 1])
			end

			for _, stream in ipairs(streams) do
				local commands_len = stream.frame_sizes[frame_index + 1] - frame_header_size
				if commands_len <= 0 then
					draw_stream_frame(gpu, file, stream, frame_index) --ensures we skip the header
					goto continue
				end

				if gpu.getScreen() ~= stream.surface.screen_addr then
					gpu.bind(stream.surface.screen_addr, false)
					if not ops.no_back then
						gpu.bitblt(back, nil, nil, nil, nil, 0)
					end
				end

				local command_count = draw_stream_frame(gpu, file, stream, frame_index)

				if ops.fps then
					gpu.setBackground(0xff0000)
					gpu.setForeground(0xffffff)
					local now, now_up = os.clock(), computer.uptime()
					local frame_elapsed, video_elapsed_up = now - frame_begin_time, now_up - video_begin_time_up
					local frame_time = frame_index / frame_rate
					gpu.set(1, 1, ("%04i %s %04.1flag %04.ffps %05.fms %05ib %04icmds"):format(
						frame_index,
						time_fmt(frame_time),
						frame_rate == 0 and 0 or frame_time - video_elapsed_up,
						1 / frame_elapsed,
						frame_elapsed * 1000,
						seek_table[frame_index + 1] - (seek_table[frame_index] or 0),
						command_count
					))
				end
				if not ops.no_back then
					gpu.bitblt()
				end
				if ops.diff then
					gpu.setBackground(0x000000)
					gpu.setForeground(0xff0000)
					gpu.fill(1, 1, stream.size_x, stream.size_y, "*")
				end

				::continue::
			end

			if not ops.fast and frame_rate ~= 0 then
				repeat
					local current_time = (computer.uptime() - video_begin_time_up)
					local next_frame_index = math.ceil(current_time * frame_rate)
				until next_frame_index > frame_index
			end

			while true do
				local e = event.pull(0)
				if e == nil then
					break
				elseif e == "interrupted" then
					return false
				end
			end

			frame_index = frame_index + 1
		end
		return true
	end

	if ops.loop then
		while draw() do
			file:seek("set", frames_begin_pos)
		end
	else
		draw()
	end

	if not ops.no_back then
		gpu.freeBuffer(back)
		gpu.setActiveBuffer(0)
	end
	if gpu.getScreen() ~= main_screen then
		gpu.bind(main_screen, false)
	end
end

if ops.h or ops.help then
	print("-h --help", "show this help")
	print("-v --version", "show player version")
	print("-p --probe", "show the header info")
	print("   --fps", "show performance stats during playback")
	print("   --loop", "loop video like a gif")
	print("   --no-back", "disable double buffering and the dependency on GPU buffers")
	print("   --cfg", "set the screen layout and other environment settings. generate a configs with 'screenConfig.lua'")
	print("   --diff", "only draw what changed from the last frame")
	print("   --seek", "skip frames to ensure real-time playback (buggy due to no I-frames in format)")
	print("   --fast", "don't wait for frame time; render next frame as fast as possible")
	print("   --batch-check", "debug the batches")
	print("   --color-check", "debug the colors")
	return
end
if ops.v or ops.version then
	print(("SZT Stream Reader V%s"):format(version))
	return
end

--open file
local gpu = component.gpu

--get surfaces
local surfaces
if ops.cfg then
	local file, reason = io.open(ops.cfg, "r")
	if not file then
		error("Failed opening config file for reading: " .. reason)
	end
	surfaces = serialization.unserialize(file:read("*a"))
	file:close()
else
	surfaces = {
		main = {
			screen_addr = gpu.getScreen(),
			is_fullscreen = true,
		}
	}
end

--do playback
local path = args[1]
local file, reason = linear_stream.open(path, "rb")
if not file then
	error("Failed to open file: " .. reason)
end


local ok, reason = xpcall(function()
	if ops.p or ops.probe then
		probe_header(file)
		return
	end

	render(gpu, file, surfaces)
end, function(err)
	return ("%s | %s"):format(err, debug.traceback())
end)
file:close()
if ops.p or ops.probe then return end

--do cleanup
if not ops.no_back then
	gpu.setActiveBuffer(0)
end
gpu.setBackground(0xff0000)
gpu.setForeground(0xffffff)
term.setCursor(1, 1)

--handle error
if not ok then
	if not ops.no_back then
		gpu.freeAllBuffers()
	end
	print("ERR:")
	print(reason)
	return
end

print("Done!")
