local os = require("os")
local io = require("io")
local shell = require("shell")
local event = require("event")
local unicode = require("unicode")
local component = require("component")
local computer = require("computer")
local term = require("term")
local serialization = require("serialization")

local version = "2.0"

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
	return string.unpack("<I2", file:read(2))
end
local function read_u32(file)
	return string.unpack("<I4", file:read(4))
end
local function read_u64(file)
	return string.unpack("<I8", file:read(8))
end

local function check_interrupted()
	while true do
		local e = event.pull(0)
		if e == nil then
			break
		elseif e == "interrupted" then
			return true
		end
	end
	return false
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
	version = 4,
}

local args, ops = shell.parse(...)
ops.no_back = ops["no-back"]
ops.batch_check = ops["batch-check"]
ops.color_check = ops["color-check"]

local video = {}
do
	local main_screen
	local back
	function video.pre_init(gpu)
		main_screen = gpu.getScreen()
	end

	function video.init(gpu, size_x, size_y)
		if gpu.getScreen() ~= main_screen then
			gpu.bind(main_screen, false)
		end

		if not ops.no_back and size_x > 0 and size_y > 0 then
			back = gpu.allocateBuffer(size_x, size_y)
			if back == nil then error("can't allocate back-buffer") end

			gpu.setActiveBuffer(back)
		end
	end

	function video.deinit(gpu)
		if back then
			gpu.freeBuffer(back)
			gpu.setActiveBuffer(0)
		end
		if gpu.getScreen() ~= main_screen then
			gpu.bind(main_screen, false)
		end
	end


	local lut = {
		0x0f0f0f, 0x1e1e1e, 0x2d2d2d, 0x3c3c3c, 0x4b4b4b, 0x5a5a5a, 0x696969, 0x787878, 0x878787, 0x969696, 0xa5a5a5, 0xb4b4b4, 0xc3c3c3, 0xd2d2d2, 0xe1e1e1, 0xf0f0f0,
		0x000000, 0x000040, 0x000080, 0x0000c0, 0x0000ff,  0x002400, 0x002440, 0x002480, 0x0024c0, 0x0024ff,  0x004900, 0x004940, 0x004980, 0x0049c0, 0x0049ff,  0x006d00, 0x006d40, 0x006d80, 0x006dc0, 0x006dff,  0x009200, 0x009240, 0x009280, 0x0092c0, 0x0092ff,  0x00b600, 0x00b640, 0x00b680, 0x00b6c0, 0x00b6ff,  0x00db00, 0x00db40, 0x00db80, 0x00dbc0, 0x00dbff,  0x00ff00, 0x00ff40, 0x00ff80, 0x00ffc0, 0x00ffff,
		0x330000, 0x330040, 0x330080, 0x3300c0, 0x3300ff,  0x332400, 0x332440, 0x332480, 0x3324c0, 0x3324ff,  0x334900, 0x334940, 0x334980, 0x3349c0, 0x3349ff,  0x336d00, 0x336d40, 0x336d80, 0x336dc0, 0x336dff,  0x339200, 0x339240, 0x339280, 0x3392c0, 0x3392ff,  0x33b600, 0x33b640, 0x33b680, 0x33b6c0, 0x33b6ff,  0x33db00, 0x33db40, 0x33db80, 0x33dbc0, 0x33dbff,  0x33ff00, 0x33ff40, 0x33ff80, 0x33ffc0, 0x33ffff,
		0x660000, 0x660040, 0x660080, 0x6600c0, 0x6600ff,  0x662400, 0x662440, 0x662480, 0x6624c0, 0x6624ff,  0x664900, 0x664940, 0x664980, 0x6649c0, 0x6649ff,  0x666d00, 0x666d40, 0x666d80, 0x666dc0, 0x666dff,  0x669200, 0x669240, 0x669280, 0x6692c0, 0x6692ff,  0x66b600, 0x66b640, 0x66b680, 0x66b6c0, 0x66b6ff,  0x66db00, 0x66db40, 0x66db80, 0x66dbc0, 0x66dbff,  0x66ff00, 0x66ff40, 0x66ff80, 0x66ffc0, 0x66ffff,
		0x990000, 0x990040, 0x990080, 0x9900c0, 0x9900ff,  0x992400, 0x992440, 0x992480, 0x9924c0, 0x9924ff,  0x994900, 0x994940, 0x994980, 0x9949c0, 0x9949ff,  0x996d00, 0x996d40, 0x996d80, 0x996dc0, 0x996dff,  0x999200, 0x999240, 0x999280, 0x9992c0, 0x9992ff,  0x99b600, 0x99b640, 0x99b680, 0x99b6c0, 0x99b6ff,  0x99db00, 0x99db40, 0x99db80, 0x99dbc0, 0x99dbff,  0x99ff00, 0x99ff40, 0x99ff80, 0x99ffc0, 0x99ffff,
		0xcc0000, 0xcc0040, 0xcc0080, 0xcc00c0, 0xcc00ff,  0xcc2400, 0xcc2440, 0xcc2480, 0xcc24c0, 0xcc24ff,  0xcc4900, 0xcc4940, 0xcc4980, 0xcc49c0, 0xcc49ff,  0xcc6d00, 0xcc6d40, 0xcc6d80, 0xcc6dc0, 0xcc6dff,  0xcc9200, 0xcc9240, 0xcc9280, 0xcc92c0, 0xcc92ff,  0xccb600, 0xccb640, 0xccb680, 0xccb6c0, 0xccb6ff,  0xccdb00, 0xccdb40, 0xccdb80, 0xccdbc0, 0xccdbff,  0xccff00, 0xccff40, 0xccff80, 0xccffc0, 0xccffff,
		0xff0000, 0xff0040, 0xff0080, 0xff00c0, 0xff00ff,  0xff2400, 0xff2440, 0xff2480, 0xff24c0, 0xff24ff,  0xff4900, 0xff4940, 0xff4980, 0xff49c0, 0xff49ff,  0xff6d00, 0xff6d40, 0xff6d80, 0xff6dc0, 0xff6dff,  0xff9200, 0xff9240, 0xff9280, 0xff92c0, 0xff92ff,  0xffb600, 0xffb640, 0xffb680, 0xffb6c0, 0xffb6ff,  0xffdb00, 0xffdb40, 0xffdb80, 0xffdbc0, 0xffdbff,  0xffff00, 0xffff40, 0xffff80, 0xffffc0, 0xffffff,
	}
	
	local chars = {}
	local function gv_raw(file, len) return file:read(len) end
	local function gv_braille(file, len)
		for i = 1, len do
			chars[i] = unicode.char(0x2800 + read_u8(file))
		end
		for j = len + 1, #chars do
			chars[j] = nil
		end
		return table.concat(chars)
	end

	local frame_header_size = 1 -- 1 from the command_kind (this is a constant)
	function video.draw_stream_frame(gpu, file, stream, commands_len)
		local pos_x, pos_y = stream.surface.pos_x, stream.surface.pos_y

		local command_kind = read_u8(file)

		local get_value =
			command_kind == 0x01 and gv_braille or --check 0x01 first since it's likely more common
			command_kind == 0x00 and gv_raw or
			error(("bad frame command_kind '%i'"):format(command_kind))

		if ops.batch_check then
			get_value = function(file, len)
				file:read(len) --more efficient than seek
				gpu.setBackground(math.random(0xffffff))
				return (" "):rep(len)
			end
		elseif ops.color_check then
			get_value = function(file, len, i)
				file:read(len) --more efficient than seek
				gpu.setBackground((i==0 and 0x010101 or i==1 and 0x000100 or 0x010000)*math.max(1, math.ceil(math.ceil(len*8/0x40)/8*0xff)))
				return (" "):rep(len)
			end
		end

		local command_count = 0
		local i = 0

		local len, x, y
		while i < commands_len do
			len = read_u8(file)
			local q = i
			if len >= 0x80 then
				len = len - 0x80
				gpu.setBackground(lut[read_u8(file) + 1])
				i = i + 1
			end
			if len >= 0x40 then
				len = len - 0x40
				gpu.setForeground(lut[read_u8(file) + 1])
				i = i + 1
			end

			len = len + 1
			x = read_u8(file)
			y = read_u8(file)
			gpu.set(x + pos_x, y + pos_y, get_value(file, len, i-q))
			i = i + 3 + len
			command_count = command_count + 1
		end
		return command_count
	end
end

local audio = {}
do --audio engine
	local num_channels = 8
	local sound_engines = {}
	function audio.init(num_voices)
		for addr, _ in pairs(component.list("sound")) do
			local engine = component.proxy(addr)
			table.insert(sound_engines, engine)
			--print(engine.address, engine)

			engine.setTotalVolume(1)
			for channel = 1, num_channels do
				engine.close(channel)
			end
		end

		for voice=1, num_voices do
			local engine_index = math.floor((voice - 1) / num_channels)
			local engine = sound_engines[engine_index + 1]
			local channel = voice - engine_index * num_channels
			engine.open(channel)
			engine.setWave(channel, engine.modes.sine)
			engine.resetFM(channel)
			engine.resetAM(channel)
			engine.resetEnvelope(channel)
		end
	end

	function audio.deinit(num_voices)
		for voice = 1, num_voices do
			local engine_index = math.floor((voice - 1) / num_channels)
			local engine = sound_engines[engine_index + 1]
			local channel = voice - engine_index * num_channels
			engine.close(channel)
		end
		
		for _, engine in ipairs(sound_engines) do
			engine.process()
		end
	end

	local buf_time = 0
	local num_instructions = 0
	local prev_proc_life = 0
	local buf_min = 150
	local buf_max = 1000
	local prev_proc_time = computer.uptime()
	function audio.commit(t, num_voices, gpu)
		for i, engine in ipairs(sound_engines) do
			if (i - 1) * num_channels >= num_voices then break end
			engine.delay(t);
		end
		buf_time = buf_time + t
		num_instructions = num_instructions + 1
		local now = computer.uptime()
		local time_since_proc = (now - prev_proc_time) * 1000
		local time_to_death = prev_proc_life - time_since_proc
		local ready_life = time_to_death + buf_time
		if ready_life > buf_max then
			local correction = ready_life - buf_max
			--print(("Correction: %fms"):format(correction))
			os.sleep(correction / 1000);
			time_since_proc = time_since_proc + correction
			time_to_death = time_to_death - correction
			ready_life = ready_life - correction
		end
		--if time_to_death < buf_min and ready_life >= buf_min then
		if time_to_death < buf_min then
			for i, engine in ipairs(sound_engines) do
				if (i - 1) * num_channels >= num_voices then break end
				engine.process()
			end
			buf_time = 0

			if ops.fps and gpu then
				gpu.setBackground(0xff0000)
				gpu.setForeground(0xffffff)
				gpu.set(1, 2, ("INS:%03i DELTA:%06.1fms LIFE:%06.1fms"):format(num_instructions, time_since_proc, ready_life))
			end
			num_instructions = 0
			prev_proc_time = now
			prev_proc_life = ready_life
		end
	end

	function audio.play(file, num_voices, gpu)
		for voice = 1, num_voices do
			local engine_index = math.floor((voice - 1) / num_channels)
			local engine = sound_engines[engine_index + 1]
			local channel = voice - engine_index * num_channels
			engine.setVolume(channel, read_u8(file) / 0xff)
			engine.setFrequency(channel, read_u16(file) / 0xffff * 20000)
		end
		audio.commit(read_u8(file), num_voices, gpu)
	end
end

local function read_header(file)
	local magic = file:read(4)
	local version = read_u16(file)
	local num_streams = read_u8(file)

	local streams = {}
	for i = 1, num_streams do
		local stream = {
			kind = read_u8(file),
			num_packets = read_u32(file),
			name = file:read(read_u8(file)),
		}
		if stream.kind == 0x00 then
			stream.frame_rate = read_u16(file)
			stream.size_x = read_u8(file)
			stream.size_y = read_u8(file)
		elseif stream.kind == 0x01 then
			stream.num_voices = read_u8(file)
		end
		table.insert(streams, stream)
	end

	return {
		magic = magic,
		version = version,
		streams = streams,
	}
end

local function probe_header(file)
	local header = read_header(file)
	print(("magic: %s %s"):format(header.magic, header.magic == szt.magic and "OK" or "ERR"))
	print(("version: %i %s"):format(header.version,
		header.version == szt.version and "OK" or
		header.version > szt.version and "NEW" or
		"OLD"
	))

	print(("found %i streams:"):format(#header.streams))
	for i, stream in ipairs(header.streams) do
		local kind_text =
			stream.kind == 0x00 and "video" or
			stream.kind == 0x01 and "audio" or
			"unknown"

		print(("  %i: %s | '%s'"):format(i, kind_text, stream.name))
		print(("    num_packets: %i"):format(stream.num_packets))
		if stream.kind == 0x00 then
			print(("    frame_rate: %i"):format(stream.frame_rate))
			print(("    size: %ix%i"):format(stream.size_x, stream.size_y))
		elseif stream.kind == 0x01 then
			print(("    num_voices: %i"):format(stream.num_voices))
		end
	end

	local frames_begin_pos = file:seek()
	print(("headers done at byte: %i"):format(frames_begin_pos))
end

local function play(gpu, file, surfaces)
	local header = read_header(file)
	assertEq(header.magic, szt.magic, "bad magic")
	print("magic: OK")
	assertEq(header.version, szt.version, "bad version")
	print("version: OK")

	local num_streams = header.num_streams

	video.pre_init(gpu)
	local max_size_x, max_size_y = 0, 0
	local max_num_voices = 0
	local streams = {}
	for i, stream_desc in ipairs(header.streams) do
		local stream = {
			kind = stream_desc.kind,
			num_packets = stream_desc.num_packets,
			name = stream_desc.name,
		}
		if stream_desc.kind == 0x00 then
			local surface = surfaces[stream_desc.name] or error(("missing surface for stream '%s'"):format(stream_desc.name))
			surface.pos_x = surface.is_fullscreen and 1 or surface.pos_x or error("surface has no pos_x")
			surface.pos_y = surface.is_fullscreen and 1 or surface.pos_y or error("surface has no pos_y")

			stream.surface = surface
			stream.frame_rate = stream_desc.frame_rate
			stream.size_x = stream_desc.size_x
			stream.size_y = stream_desc.size_y

			max_size_x = math.max(max_size_x, stream.size_x)
			max_size_y = math.max(max_size_y, stream.size_y)

			if stream.surface.is_fullscreen then
				gpu.bind(stream.surface.screen_addr, false)
				gpu.setResolution(stream.size_x, stream.size_y)
			end
		elseif stream_desc.kind == 0x01 then
			stream.num_voices = stream_desc.num_voices
			max_num_voices = math.max(max_num_voices, stream.num_voices)
		end
		table.insert(streams, stream)
	end

	local frames_begin_pos = file:seek()
	video.init(gpu, max_size_x, max_size_y)
	audio.init(max_num_voices)

	local function play_impl()
		local video_begin_time, video_begin_time_up = os.clock(), computer.uptime()
		local num_total_packets = 0
		for _, stream in ipairs(streams) do
			num_total_packets = num_total_packets + stream.num_packets
		end

		local packet_index = 0
		while packet_index < num_total_packets do
			local frame_begin_time, frame_begin_time_up = os.clock(), computer.uptime()

			local stream_id = read_u8(file)
			local stream = streams[stream_id + 1]
			if stream.kind == 0x00 then
				local commands_len = read_u16(file)
				if commands_len > 0 then
					if gpu.getScreen() ~= stream.surface.screen_addr then
						gpu.bind(stream.surface.screen_addr, false)
						if back then
							gpu.bitblt(back, nil, nil, nil, nil, 0)
						end
					end
				end

				local command_count = video.draw_stream_frame(gpu, file, stream, commands_len)

				if commands_len > 0 then
					if ops.fps then
						local now, now_up = os.clock(), computer.uptime()
						local frame_elapsed, video_elapsed_up = now - frame_begin_time, now_up - video_begin_time_up
						local frame_time = packet_index / (stream.frame_rate or 0)
						local packet_len = commands_len + 4
						gpu.setBackground(0xff0000)
						gpu.setForeground(0xffffff)
						gpu.set(1, 1, ("%04i %s %04.1flag %04.ffps %05.fms %05ib %04icmds"):format(
							packet_index,
							time_fmt(frame_time),
							stream.frame_rate == 0 and 0 or video_elapsed_up - frame_time,
							1 / frame_elapsed,
							frame_elapsed * 1000,
							packet_len,
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
				end
			elseif stream.kind == 0x01 then
				audio.play(file, stream.num_voices, gpu)
			end

			if not ops.fast and stream.frame_rate ~= 0 and stream.frame_rate ~= nil then
				repeat
					local current_time = (computer.uptime() - video_begin_time_up)
					local next_frame_index = math.ceil(current_time * stream.frame_rate)
				until next_frame_index > packet_index
			end

			if check_interrupted() then
				return false
			end

			packet_index = packet_index + 1
		end
		return true
	end

	if ops.loop then
		while play_impl() do
			file:seek("set", frames_begin_pos)
		end
	else
		play_impl()
	end

	video.deinit(gpu)
	audio.deinit(max_num_voices)
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
	print("   --fast", "don't wait for frame time; render next frame as fast as possible")
	print("   --batch-check", "debug the batches")
	print("   --color-check", "debug the colors")
	return
end
if ops.v or ops.version then
	print(("SZT Stream Reader V%s"):format(version))
	return
end

local gpu = component.gpu
-- local dummy = function() return 0 end
-- local gpu = {
-- 	__index = function() return dummy end
-- }
-- setmetatable(gpu, gpu)

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

local function run()
	if ops.p or ops.probe then
		probe_header(file)
		return
	end

	play(gpu, file, surfaces)
end

local res_x, res_y = gpu.getResolution()
local function errHandler(err)
	gpu.setBackground(0xff0000)
	gpu.setForeground(0xffffff)
	return debug.traceback(err)
end

local result, reason = xpcall(run, errHandler)

file:close()
if ops.p or ops.probe then return end

--do cleanup
if not ops.no_back then
	gpu.setActiveBuffer(0)
end
gpu.setResolution(res_x, res_y)

--handle error
if not result then
	if not ops.no_back then
		gpu.freeAllBuffers()
	end
	print("ERR:")
	print(reason)
	return
end

print("Done!")
