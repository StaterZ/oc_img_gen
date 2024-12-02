local os = require("os")
local shell = require("shell")
local event = require("event")
local unicode = require("unicode")
local component = require("component")
local term = require("term")
local serialization = require("serialization")

local args, ops = shell.parse(...)

local function assertEq(found, expected, msg)
	assert(found == expected, ("%s: expected '%s', found '%s'"):format(msg, expected, format))
end

function read_u8(file)
	return file:read(1):byte()
end

function read_u16(file)
	return
		read_u8(file) * 0x100 +
		read_u8(file)
end

function read_u32(file)
	return
		read_u16(file) * 0x10000 +
		read_u16(file)
end

function read_u64(file)
	return
		read_u32(file) * 0x100000000 +
		read_u32(file)
end

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

local frame_header_size = 1 -- 1 from the command_kind
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

	local commands_len = stream.frame_sizes[frame_index + 1] - frame_header_size
	local i = 0
	while i < commands_len do
		local len = read_u8(file)
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
		local x = read_u8(file)
		local y = read_u8(file)
		gpu.set(x + pos_x, y + pos_y, get_value(len))
		i = i + 3 + len
	end
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

local function render(gpu, file, surfaces)
	local header = read_header(file)
	assertEq(header.magic, "sztb", "bad magic")
	print("magic: OK")
	assertEq(header.version, 3, "bad version")
	print("version: OK")

	local frame_rate = header.frame_rate
	local num_frames = header.num_frames
	local num_streams = header.num_streams

	print(("reading %i stream descriptors:"):format(num_streams))

	for i, stream in ipairs(header.streams) do
		print(("%4i: '%s' %ix%i"):format(
			i,
			stream.name,
			stream.size_x,
			stream.size_y
		))
	end

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
	print(("headers done at byte %i"):format(frames_begin_pos))

	local back
	if not ops.noback then
		back = gpu.allocateBuffer(max_size_x, max_size_y)
		if back == nil then error("can't allocate back-buffer") end

		gpu.setActiveBuffer(back)
	end

	local function draw()
		local begin_time = os.clock()
		local frame_index = 0
		while frame_index < num_frames do
			local frame_begin_time = os.clock()

			if ops.seek then
				local current_time = (os.clock() - begin_time)
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
					if not ops.noback then
						gpu.bitblt(back, nil, nil, nil, nil, 0)
					end
				end

				draw_stream_frame(gpu, file, stream, frame_index)

				if ops.fps then
					gpu.setBackground(0xff0000)
					gpu.setForeground(0xffffff)
					local now = os.clock()
					local elapsed = now - frame_begin_time
					gpu.set(1, 1, ("%04i %04.1flag %03.ffps %05.fms %05ib"):format(
						frame_index,
						frame_rate == 0 and 0 or frame_index / frame_rate - (now - begin_time),
						1 / elapsed,
						elapsed * 1000,
						seek_table[frame_index + 1] - (seek_table[frame_index] or 0)
					))
				end
				if not ops.noback then
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
					local current_time = (os.clock() - begin_time)
					local next_frame_index = math.ceil(current_time * frame_rate)
				until next_frame_index > frame_index
			end

			while true do
				local e = event.pull(0)
				if e == nil then
					break
				elseif e == "interupted" then
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

	if not ops.noback then
		gpu.freeBuffer(back)
		gpu.setActiveBuffer(0)
	end
	if gpu.getScreen() ~= main_screen then
		gpu.bind(main_screen, false)
	end
end

--open file
local gpu = component.gpu

--get surfaces
local surfaces
if ops.cfg then
	local file, reason = io.open(args[2], "r")
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
local file, reason = io.open(path, "rb")
if not file then
	error("Failed to open file: " .. reason)
end

local ok, reason = xpcall(function() render(gpu, file, surfaces) end, function(err)
	return ("%s | %s"):format(err, debug.traceback())
end)
file:close()

--do cleanup
if not ops.noback then
	gpu.setActiveBuffer(0)
end
gpu.setBackground(0xff0000)
gpu.setForeground(0xffffff)
term.setCursor(1, 1)

--handle error
if not ok then
	if not ops.noback then
		gpu.freeAllBuffers()
	end
	print("ERR:")
	print(reason)
	return
end

print("Done!")
