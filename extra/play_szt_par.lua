local os = require("os")
local io = require("io")
local fs = require("filesystem")
local shell = require("shell")
local unicode = require("unicode")
local component = require("component")
local computer = require("computer")
local serialization = require("serialization")

local version = "4.3-par"

--the player names the tape it burns pcm audio onto, so it can find it again
local burner_label = "szt pcm burner"

--the primary drive, unless it holds the burner tape, then another drive
local function default_tape_drive()
	if not component.isAvailable("tape_drive") then
		return nil, "no tape drive found"
	end
	if component.tape_drive.getLabel() == burner_label then
		for candidate in component.list("tape_drive") do
			local d = component.proxy(candidate)
			if d.isReady() and d.getLabel() ~= burner_label then
				return candidate
			end
		end
	end
	return component.tape_drive.address
end

--raw stream over a computronics tape drive, the data starts at the beginning of the tape
local tape_stream = {}
do
	local mt = {
		__index = tape_stream,
		__metatable = "TapeStream"
	}

	function tape_stream.open(address)
		local addr, reason
		if address then
			addr, reason = component.get(address, "tape_drive")
		else
			addr, reason = default_tape_drive()
		end
		if not addr then return nil, reason end
		local drive = component.proxy(addr)
		if not drive.isReady() then return nil, "no tape in drive" end
		drive.stop() --a playing tape moves under us

		return setmetatable({
			drive = drive,
			pos = 0,
			drive_pos = drive.getPosition(),
			size = drive.getSize(),
		}, mt)
	end

	function tape_stream:close() end

	--drive.seek costs a tick like drive.read, so only move the tape when we read
	function tape_stream:seek(whence, offset)
		if whence == "set" then
			self.pos = offset
		elseif whence == "cur" then
			self.pos = self.pos + offset
		else
			return nil, "can't seek from the end of a tape"
		end
		return self.pos
	end

	function tape_stream:read(n)
		local left = self.size - self.pos
		if left <= 0 then return nil end
		n = math.floor(math.min(n, left)) --the drive wants an integer
		if self.pos ~= self.drive_pos then
			self.drive.seek(self.pos - self.drive_pos)
		end
		local data = self.drive.read(n)
		if not data then return nil, "no tape in drive" end
		self.pos = self.pos + #data
		self.drive_pos = self.pos
		return data
	end
end

local linear_stream = {}
do
	local mt = {
		__index = linear_stream,
		__metatable = "LinearStream"
	}

	function linear_stream.open(path, format, is_tape)
		assert(format == "rb")
		path = type(path) == "string" and {path} or path
		if is_tape and #path == 0 then path = {false} end --primary drive

		--each tape read costs a tick, so read more at once
		local bufferSize = math.floor(math.max(512, math.min((is_tape and 32 or 2) * 1024, computer.freeMemory() / 8)))

		local streams = {}
		for i, p in ipairs(path) do
			local stream, reason
			if is_tape then
				stream, reason = tape_stream.open(p or nil)
			else
				p = shell.resolve(p)
				stream, reason = fs.open(p, format)
			end
			if not stream then return nil, reason end

			stream:seek("cur", (i - 1) * bufferSize)
			table.insert(streams, stream)
		end

		return setmetatable({
			streams = streams,
			next_stream = 1,
			buffer = "",
			bufferHead = 0,
			bufferSize = bufferSize,
		}, mt)
	end

	function linear_stream:close()
		self.buffer = nil
		for _, stream in ipairs(self.streams) do
			stream:close()
		end
	end

	function linear_stream:seek(whence, offset) --shamelessly stolen from OpenOS buffer code
		whence = whence or "cur"
		assert(whence == "set" or whence == "cur" or whence == "end",
			"bad argument #1 (set, cur or end expected, got " .. whence .. ")")
		offset = offset or 0
		checkArg(2, offset, "number")
		assert(math.floor(offset) == offset, "bad argument #2 (not an integer)")

		local result
		if whence == "cur" then
			offset = offset - (#self.buffer - self.bufferHead)
			for i, stream in ipairs(self.streams) do
				local pos, reason = stream:seek(whence, offset)
				if not pos then return nil, reason end
				if i == self.next_stream then result = pos end --the next stream holds the logical position
			end
		elseif whence == "set" then
			for i, stream in ipairs(self.streams) do
				local pos, reason = stream:seek(whence, offset + (i - self.next_stream) % #self.streams * self.bufferSize)
				if not pos then return nil, reason end
				if i == self.next_stream then result = pos end
			end
		else
			error("OUF!")
		end
		
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
			local result, reason = self.streams[self.next_stream]:read(self.bufferSize)
			self.streams[self.next_stream]:seek("cur", (#self.streams - 1) * self.bufferSize)
			self.next_stream = self.next_stream % #self.streams + 1
			if not result then
				if reason then
					return result, reason
				else
					error("read past EOF")
				end
			end
			self.buffer = result

			if needed > #self.buffer then
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
	assert(found == expected, ("%s: expected '%s', found '%s'"):format(msg, expected, found))
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


local next_check_time = 0
local function check_interrupted()
	local now = computer.uptime()

	if now < next_check_time then return false end
	next_check_time = now + 1

	while true do
		local e = computer.pullSignal(0)
		if not e then
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
	version = 7,
}

local args, ops = shell.parse(...)
ops.tape = ops.tape or #args == 0 --with no file, play the primary tape drive
ops.no_video = ops["no-video"]
ops.no_audio = ops["no-audio"]
ops.no_back = ops["no-back"]
ops.one_buf = ops["one-buf"]
ops.batch_check = ops["batch-check"]
ops.pcm_tape = ops["pcm-tape"]
ops.pcm_format = ops["pcm-format"]
ops.volume = ops.volume or 1

local stats = {}
function draw_stats(gpu)
	gpu.setBackground(0xff0000)
	gpu.setForeground(0xffffff)
	for i, text in pairs(stats) do
		gpu.set(1, i, text)
	end
end

local video = {}
do --video player
	local buffer_pool = {}
	local buffer_pool_idx = {}
	local buffer_pool_size
	local function pool_insert(buffer)
		buffer_pool[#buffer_pool + 1] = buffer
		buffer_pool_idx[buffer] = #buffer_pool
	end

	-- Remove a specific buffer from the pool in O(1). Returns true if the
	-- buffer was in the pool, false if not.
	local function pool_remove(buffer)
		local idx = buffer_pool_idx[buffer]
		if not idx then return false end

		local last_idx = #buffer_pool
		local last_buffer = buffer_pool[last_idx]
		buffer_pool[idx] = last_buffer
		buffer_pool_idx[last_buffer] = idx
		buffer_pool[last_idx] = nil
		buffer_pool_idx[buffer] = nil
		return true
	end

	local function pool_pop()
		local last_idx = #buffer_pool
		local last_buffer = buffer_pool[last_idx]
		buffer_pool[last_idx] = nil
		buffer_pool_idx[last_buffer] = nil
		return last_buffer
	end

	local main_screen
	function video.pre_init(gpu)
		main_screen = gpu.getScreen()
	end

	function video.bind_main_screen(gpu)
		if gpu.getScreen() ~= main_screen then
			gpu.bind(main_screen, false)
		end
	end

	function video.init(gpu, size_x, size_y)
		video.bind_main_screen(gpu)

		if not ops.no_back then
			if size_x > 0 and size_y > 0 then
				repeat
					local buffer = gpu.allocateBuffer(size_x, size_y)
					if not buffer then break end

					pool_insert(buffer)
				until ops.one_buf -- the sallad approves
			end
			if #buffer_pool == 0 then
				video.deinit(gpu)
				error("can't allocate back-buffer")
			end
		end
		buffer_pool_size = #buffer_pool
		return buffer_pool_size
	end

	function video.deinit(gpu)
		if buffer_pool then
			for _, buffer in ipairs(buffer_pool) do
				gpu.freeBuffer(buffer)
			end
			gpu.setActiveBuffer(0)
		end
		video.bind_main_screen(gpu)
	end

	local buffer_cache = {}
	local buffer_cache_idx = {}
	local function alloc_buffer(cache)
		-- 1. try to reuse the cache
		if pool_remove(cache) then
			return cache
		end

		-- 2. try to find a free buffer (not used for cache)
		for _, candidate in ipairs(buffer_pool) do
			if not buffer_cache_idx[candidate] then
				pool_remove(candidate)
				return candidate
			end
		end

		-- 3. worst case, pick anything
		return pool_pop()
	end

	function video.begin_commit(gpu, track)
		local cache = buffer_cache[track.index + 1]
		if not cache then --if no cache, create a new back-buffer by copying the screen
			if gpu.getScreen() ~= track.surface.screen_addr then
				gpu.bind(track.surface.screen_addr, false) --OUCH!
			end
			cache = 0
		end
		
		local buffer = alloc_buffer(cache)
		if buffer ~= cache then --if we could not get the cache, then copy it
			gpu.bitblt(
				buffer,
				track.surface.pos_x,
				track.surface.pos_y,
				track.surface.size_x,
				track.surface.size_y,
				cache
			)
		end
		gpu.setActiveBuffer(buffer)
		
		--update bookkeeping
		local prev_buffer_idx = buffer_cache_idx[buffer]
		if prev_buffer_idx then
			buffer_cache[prev_buffer_idx] = nil
		end
		buffer_cache[track.index + 1] = buffer
		buffer_cache_idx[buffer] = track.index + 1
		table.insert(track.swap_chain, buffer)
	end

	function video.begin_dummy_commit(track)
		table.insert(track.swap_chain, 0)
	end

	function video.inject(gpu, track, f)
		local active = gpu.getActiveBuffer()
		gpu.setActiveBuffer(track.swap_chain[1])
		f()
		gpu.setActiveBuffer(active)
	end

	function video.present(gpu, track)
		local frame = table.remove(track.swap_chain, 1)
		if frame == 0 then return end --delete dummy commits

		gpu.bitblt(
			0,
			track.surface.pos_x,
			track.surface.pos_y,
			track.surface.size_x,
			track.surface.size_y,
			frame
		)
		pool_insert(frame)
	end

	function video.is_stalled()
		return #buffer_pool == 0
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
	
	--unicode.char is a native call, so build all 256 braille chars once
	local braille = {}
	for b = 0, 0xff do
		braille[b] = unicode.char(0x2800 + b)
	end

	local byte, sub, rep, concat = string.byte, string.sub, string.rep, table.concat

	local concat_buffer = {}

	local frame_header_size = 1 -- 1 from the command_kind (this is a constant)
	function video.draw_track_frame(gpu, file, track, commands_len)
		local pos_x, pos_y = track.surface.pos_x, track.surface.pos_y

		local command_kind = read_u8(file)
		local is_braille = command_kind == 0x01 --check 0x01 first since it's likely more common
		if not is_braille and command_kind ~= 0x00 then
			error(("bad frame command kind '%x2'"):format(command_kind))
		end
		local batch_check = ops.batch_check

		--read the whole frame at once and parse it with byte() instead of a read per field
		local data = file:read(commands_len)
		local gpu_set, gpu_set_bg, gpu_set_fg = gpu.set, gpu.setBackground, gpu.setForeground

		local command_count = 0
		local i = 1

		local len, x, y, value
		while i <= commands_len do
			len = byte(data, i)
			i = i + 1
			if len >= 0x80 then
				len = len - 0x80
				gpu_set_bg(lut[byte(data, i) + 1])
				i = i + 1
			end
			if len >= 0x40 then
				len = len - 0x40
				gpu_set_fg(lut[byte(data, i) + 1])
				i = i + 1
			end
			local is_rle = len >= 0x20
			if is_rle then
				len = len - 0x20
			end

			len = len + 1
			x, y = byte(data, i, i + 1)
			i = i + 2
			if batch_check then
				gpu_set_bg(math.random(0xffffff))
				value = rep(" ", len)
			elseif not is_braille then
				value = sub(data, i, i + len - 1)
			elseif is_rle then
				if len == 1 then
					value = rep(braille[byte(data, i)], byte(data, i + 1))
				else
					local q = i
					for j = 1, len do
						concat_buffer[j] = rep(braille[byte(data, q)], byte(data, q + 1))
						q = q + 2
					end
					value = concat(concat_buffer, "", 1, len)
				end
			else
				if len == 1 then
					value = braille[byte(data, i)]
				else
					local q = i - 1
					for j = 1, len do
						concat_buffer[j] = braille[byte(data, q + j)]
					end
					value = concat(concat_buffer, "", 1, len)
				end
			end
			gpu_set(x + pos_x, y + pos_y, value)
			i = i + (is_rle and len*2 or len)
			command_count = command_count + 1
		end
		return command_count
	end

	function video.update_stats(
		track,
		frame_begin_time,
		video_begin_time_up,
		commands_len,
		command_count
	)
		local now, now_up = os.clock(), computer.uptime()
		local frame_elapsed, video_elapsed_up = now - frame_begin_time, now_up - video_begin_time_up
		local presented_packet_time = track.presented_packet_index * track.time_base
		local packet_header_len = 4 --track_id = 1b, commands_len = 2b, command_kind = 1b
		local packet_len = commands_len + packet_header_len
		stats[track.index + 1] = ("V|%s: %05i %05i %s %04.1flag %irdy %i/%ipool %04.ffps %05.fms %05ib %04icmds"):format(
			track.name,
			track.packet_index,
			track.presented_packet_index,
			time_fmt(presented_packet_time),
			track.time_base == 0 and 0 or video_elapsed_up - presented_packet_time,
			#track.swap_chain,
			#buffer_pool,
			buffer_pool_size,
			1 / frame_elapsed,
			frame_elapsed * 1000,
			packet_len,
			command_count
		)
	end
end

local audio = {}
do --audio player
	local channels_per_card = 8
	local buf_min = 0.150
	local buf_max = 0.500
	
	local sound_cards
	local num_buffered_instructions
	local num_buffered_instructions_stable
	local start_time
	local buffered_time_exact
	local buffered_time
	local committed_time
	local pending_cards = {} --cards that haven't accepted the current batch yet
	local num_pending_cards = 0
	local pending_packet_index
	local voice_cards, voice_channels = {}, {}
	local voice_volumes, voice_frequencies = {}, {} --last values sent, to skip repeats
	function audio.init(num_voices)
		sound_cards = {}
		for addr, _ in pairs(component.list("sound")) do
			local card = component.proxy(addr)
			table.insert(sound_cards, card)
			--print(card.address, card)

			card.setTotalVolume(1)
			card.clear()
			for channel = 1, channels_per_card do
				card.close(channel)
			end
		end

		for voice=1, num_voices do
			local card_index = math.floor((voice - 1) / channels_per_card)
			local card = sound_cards[card_index + 1]
			local channel = voice - card_index * channels_per_card
			card.open(channel)
			card.setWave(channel, card.modes[ops.wave or "sine"])
			card.resetFM(channel)
			card.resetAM(channel)
			card.resetEnvelope(channel)
			voice_cards[voice] = card
			voice_channels[voice] = channel
			voice_volumes[voice] = nil
			voice_frequencies[voice] = nil
		end

		num_buffered_instructions = 0
		start_time = computer.uptime()
		buffered_time_exact = 0
		buffered_time = 0
		committed_time = 0
		num_pending_cards = 0
	end

	function audio.deinit(num_voices)
		for _, card in ipairs(sound_cards) do
			card.clear()
		end

		for voice = 1, num_voices do
			local card_index = math.floor((voice - 1) / channels_per_card)
			local card = sound_cards[card_index + 1]
			local channel = voice - card_index * channels_per_card
			card.close(channel)
		end
	end

	local byte = string.byte
	function audio.queue(file, track)
		local num_voices = track.num_voices
		local data = file:read(num_voices * 3)
		local volume_scale = ops.volume
		local p = 1
		for voice = 1, num_voices do
			local volume, freq_lo, freq_hi = byte(data, p, p + 2)
			p = p + 3
			--channel state persists between process() calls, so repeats are wasted instructions
			if volume ~= voice_volumes[voice] then
				voice_cards[voice].setVolume(voice_channels[voice], volume / 0xff * volume_scale)
				voice_volumes[voice] = volume
			end
			local frequency = freq_lo + freq_hi * 0x100
			if frequency ~= voice_frequencies[voice] then
				voice_cards[voice].setFrequency(voice_channels[voice], frequency / 0xffff * 20000)
				voice_frequencies[voice] = frequency
			end
		end
		
		buffered_time_exact = buffered_time_exact + track.time_base
		local delay_ms = math.floor((buffered_time_exact - buffered_time) * 1000)
		for i = 1, math.min(#sound_cards, math.ceil(track.num_voices / channels_per_card)) do
			sound_cards[i].delay(delay_ms)
		end
		buffered_time = buffered_time + delay_ms/1000
		num_buffered_instructions = num_buffered_instructions + 1
	end

	function audio.commit(track, flush)
		local now = computer.uptime()
		local play_time = now - start_time
		local committed_left = committed_time - play_time
		local buffered_left = buffered_time - play_time
		--print(("%07.1f buf --> %07.1f com"):format((buffered_left - committed_left) * 1000, committed_left * 1000))
		local num_cards = math.min(#sound_cards, math.ceil(track.num_voices / channels_per_card))
		if num_pending_cards == 0 and (committed_left < buf_min or flush) then
			for i = 1, num_cards do
				pending_cards[i] = true
			end
			num_pending_cards = num_cards
			pending_packet_index = track.packet_index
			num_buffered_instructions_stable = num_buffered_instructions
			num_buffered_instructions = 0
			committed_time = buffered_time
			--NOTE: committed_left is now out of date here!
		end
		--a card refuses process() while its last batch is still unsent, so retry those until every card took the batch
		if num_pending_cards > 0 then
			for i = 1, num_cards do
				if pending_cards[i] and sound_cards[i].process() then
					pending_cards[i] = false
					num_pending_cards = num_pending_cards - 1
				end
			end
			if num_pending_cards == 0 then
				track.presented_packet_index = pending_packet_index
			end
		end

		local buffered_left = buffered_time - play_time
		if not ops.fast then
			local correction = buffered_left - buf_max
			--if correction > 0 then print(("Correction: %fms"):format(correction)) end
			if true then
				if correction > 0 then
					os.sleep(correction)
				end
			else
				while correction > 0 do
					now = computer.uptime()
					play_time = now - start_time
					buffered_left = buffered_time - play_time
					correction = buffered_left - buf_max
				end
			end
			--NOTE: buffered_left&correction is now out of date here!
		end
	end

	function audio.update_stats(track)
		local now = computer.uptime()
		local play_time = now - start_time
		local committed_left = committed_time - play_time
		local buffered_left = buffered_time - play_time
		local function percentify(v) return (v - buf_min) / (buf_max - buf_min) * 100 end
		stats[track.index + 1] = ("A|%s: %05i %05i INS:%03i BUF:%07.1fms COM:%07.1fms/%04.0f%%"):format(
			track.name,
			track.packet_index,
			track.presented_packet_index,
			num_buffered_instructions_stable,
			(buffered_left - committed_left) * 1000,
			committed_left * 1000,
			percentify(committed_left)
		)
	end
end

local pcm = {}
do --pcm player. the audio burns onto a burner tape once, then a computronics tape drive plays the DFPWM block on its own
	local tape_sample_rate = 48000 --the drive plays 1500 bytes per 250ms at speed 1

	--a burner tape keeps the blocks it played until a new one doesn't fit after the last one. its layout:
	--  index: "sztp", u8 version, u8 num_entries, then per entry u32 checksum, u32 num_bytes, u32 sample_rate, u32 offset
	--  blocks: from index_size on, in the order they were burned. each one has pad_size of silence after it
	local burner_magic = "sztp"
	local burner_version = 1
	local index_size = 4096
	local entry_size = 16
	local max_entries = math.min(0xff, math.floor((index_size - 6) / entry_size))
	local pad_size = 3000 --the drive plays up to 250ms at a time, so it can overshoot a block

	local drive
	local block_offset --where the block starts on the drive
	local start_time

	--a drive that the packets are read from can't play at the same time, since a drive has one head
	local function video_drives(file)
		local used = {}
		for _, stream in ipairs(file.streams or {file.stream}) do
			if stream.drive then
				used[stream.drive.address] = true
			end
		end
		return used
	end

	local function seek_to(d, pos)
		d.seek(pos - d.getPosition())
	end

	--returns the entries, or nil when the tape holds something else
	local function read_index(d)
		seek_to(d, 0)
		local data = d.read(index_size)
		if data:sub(1, #burner_magic) ~= burner_magic then
			if data:find("[^\0]") then return nil end --not blank, so it isn't ours to erase
			return {}
		end
		if data:byte(5) ~= burner_version then return nil end

		local entries = {}
		for i = 1, data:byte(6) do
			local checksum, num_bytes, sample_rate, offset = string.unpack("<I4I4I4I4", data, 7 + (i - 1) * entry_size)
			entries[i] = {
				checksum = checksum,
				num_bytes = num_bytes,
				sample_rate = sample_rate,
				offset = offset,
			}
		end
		return entries
	end

	local function write_index(d, entries)
		local parts = {burner_magic, string.char(burner_version, #entries)}
		for _, entry in ipairs(entries) do
			parts[#parts + 1] = string.pack("<I4I4I4I4", entry.checksum, entry.num_bytes, entry.sample_rate, entry.offset)
		end
		seek_to(d, 0)
		d.write(table.concat(parts))
	end

	--copies the block from the szt file onto the burner tape, unless the tape holds it already. returns where it starts
	local function burn(d, file, track, frames_begin_pos)
		local entries = read_index(d)
		if not entries then
			if not ops.pcm_format then
				return nil, ("the tape in drive %s is not blank or a pcm burner tape. give --pcm-format to erase it"):format(d.address)
			end
			entries = {}
		end

		for _, entry in ipairs(entries) do
			if entry.checksum == track.checksum and entry.num_bytes == track.num_bytes and entry.sample_rate == track.sample_rate then
				return entry.offset
			end
		end

		local size = track.num_bytes + pad_size
		local last = entries[#entries]
		local offset = last and last.offset + last.num_bytes + pad_size or index_size
		if offset + size > d.getSize() or #entries >= max_entries then
			--it doesn't fit after the last block, so start over from the front
			if index_size + size > d.getSize() then
				return nil, ("the pcm audio is %i bytes, but the tape in drive %s only holds %i"):format(size, d.address, d.getSize() - index_size)
			end
			entries = {}
			offset = index_size
			write_index(d, entries) --forget the old blocks first, so a cut-off burn can't leave a bad entry
		end

		print(("burning pcm audio onto tape drive %s"):format(d.address))
		--each drive.write costs a tick, so write big chunks
		local chunk_size = math.floor(math.max(512, math.min(64 * 1024, computer.freeMemory() / 8)))
		file:seek("set", track.offset)
		seek_to(d, offset)
		local written = 0
		while written < track.num_bytes do
			local chunk = file:read(math.min(chunk_size, track.num_bytes - written))
			d.write(chunk)
			written = written + #chunk
			io.write(("\r%i/%i bytes (%3.f%%)"):format(written, track.num_bytes, written / track.num_bytes * 100))
		end
		print()
		d.write(string.rep("\x55", pad_size)) --alternating DFPWM bits hold the level
		file:seek("set", frames_begin_pos)

		entries[#entries + 1] = {
			checksum = track.checksum,
			num_bytes = track.num_bytes,
			sample_rate = track.sample_rate,
			offset = offset,
		}
		write_index(d, entries)
		return offset
	end

	--picks the drive to play from and gets the block onto it. returns false and why not when it can't
	function pcm.prepare(file, track, frames_begin_pos, reads_packets)
		local used = video_drives(file)
		local addr
		if type(ops.pcm_tape) == "string" then
			local reason
			addr, reason = component.get(ops.pcm_tape, "tape_drive")
			if not addr then return false, reason end
		else
			--take the tape named as the burner, else a blank tape, so a drive with other tapes in it is left alone.
			--a tape the video reads from can't be the burner, even when it has the name
			for candidate in component.list("tape_drive") do
				local d = component.proxy(candidate)
				if not used[candidate] and d.isReady() and d.getLabel() == burner_label then
					addr = candidate
					break
				end
			end
			if not addr then
				for candidate in component.list("tape_drive") do
					local d = component.proxy(candidate)
					if not used[candidate] and d.isReady() then
						addr = addr or candidate
						d.stop()
						if read_index(d) then
							addr = candidate
							break
						end
					end
				end
			end
			--with no packets to read, the drive that holds the file can play it
			if not addr and not reads_packets then
				addr = next(used)
			end
			if not addr then
				return false, next(used)
					and "the tape drive reads the video, so it can't play the pcm audio too. add a tape drive with a blank tape to burn the audio onto"
					or "no tape drive found"
			end
		end

		local found = component.proxy(addr)
		if not found.isReady() then return false, ("no tape in drive %s"):format(addr) end
		if used[addr] then
			if reads_packets then
				return false, ("tape drive %s reads the video, so it can't play the pcm audio too"):format(addr)
			end
			block_offset = track.offset --it holds the file, so play the block in place
		else
			found.stop()
			local offset, reason = burn(found, file, track, frames_begin_pos)
			if not offset then return false, reason end
			block_offset = offset
			if found.getLabel() ~= burner_label then
				found.setLabel(burner_label)
			end
		end

		drive = found
		drive.setVolume(math.max(0, math.min(1, tonumber(ops.volume) or 1)))
		return true, addr
	end

	--the bytes a burner tape needs to hold a block
	function pcm.burner_size(num_bytes)
		return index_size + num_bytes + pad_size
	end

	function pcm.stop()
		if drive then
			drive.stop()
		end
	end

	function pcm.deinit()
		if drive then
			drive.stop()
			drive = nil
		end
	end

	function pcm.start(track)
		drive.stop()
		seek_to(drive, block_offset)
		drive.setSpeed(track.sample_rate / tape_sample_rate)
		drive.play()
		start_time = computer.uptime()
	end

	--returns true once the block played out
	function pcm.update(track, is_only_live_track)
		local time_left = track.duration - (computer.uptime() - start_time)
		if time_left <= 0 then
			drive.stop() --the tape goes on past the block
			return true
		end
		if is_only_live_track then
			os.sleep(math.min(time_left, 0.5)) --nothing else to do, so wait but still catch interrupts
		end
		return false
	end

	function pcm.update_stats(track)
		local elapsed = computer.uptime() - start_time
		stats[track.index + 1] = ("P|%s: %s/%s %iHz x%.2f %s"):format(
			track.name,
			time_fmt(math.min(elapsed, track.duration)),
			time_fmt(track.duration),
			track.sample_rate,
			track.sample_rate / tape_sample_rate,
			drive.getState()
		)
	end
end

local function read_header(file)
	local magic = file:read(4)
	local version = read_u16(file)
	local num_tracks = read_u8(file)
	local settings = file:read(read_u16(file))

	local tracks = {}
	for i = 1, num_tracks do
		local track = {
			num_packets = read_u32(file),
			time_base_num = read_u16(file),
			time_base_denom = read_u16(file),
			name = file:read(read_u8(file)),
			kind = read_u8(file),
		}

		if track.kind == 0x00 then
			track.size_x = read_u8(file)
			track.size_y = read_u8(file)
		elseif track.kind == 0x01 then
			track.num_voices = read_u8(file)
		elseif track.kind == 0x02 then
			track.sample_rate = read_u32(file)
			track.offset = read_u32(file)
			track.num_bytes = read_u32(file)
			track.checksum = read_u32(file)
		end
		table.insert(tracks, track)
	end

	return {
		magic = magic,
		version = version,
		settings = settings,
		tracks = tracks,
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

	print("encoded with:")
	for line in header.settings:gmatch("[^\n]+") do
		print("  " .. line)
	end

	print(("found %i tracks:"):format(#header.tracks))
	for i, track in ipairs(header.tracks) do
		local kind_text =
			track.kind == 0x00 and "video" or
			track.kind == 0x01 and "audio" or
			track.kind == 0x02 and "pcm" or
			"unknown"

		print(("  %i: %s | '%s'"):format(i - 1, kind_text, track.name))
		print(("    time_base: %i/%i"):format(track.time_base_num, track.time_base_denom))
		print(("    num_packets: %i"):format(track.num_packets))
		if track.kind == 0x00 then
			print(("    size: %ix%i (effective: %ix%i)"):format(track.size_x, track.size_y, track.size_x * 2, track.size_y * 4))
		elseif track.kind == 0x01 then
			print(("    num_voices: %i"):format(track.num_voices))
		elseif track.kind == 0x02 then
			print(("    sample_rate: %iHz (tape speed %.2f)"):format(track.sample_rate, track.sample_rate / 48000))
			print(("    dfpwm: %i bytes at byte %i (%s)"):format(track.num_bytes, track.offset, time_fmt(track.num_bytes * 8 / track.sample_rate)))
			print(("    checksum: %08x"):format(track.checksum))
		end
	end

	local frames_begin_pos = file:seek()
	print(("headers done at byte: %i"):format(frames_begin_pos))

	--pcm blocks end the file, otherwise walk the packets to find the end
	local file_size = frames_begin_pos
	local pcm_tracks = {}
	local num_packets = 0
	for _, track in ipairs(header.tracks) do
		num_packets = num_packets + track.num_packets
		if track.kind == 0x02 then
			file_size = math.max(file_size, track.offset + track.num_bytes)
			table.insert(pcm_tracks, track)
		end
	end
	if #pcm_tracks == 0 then
		for _ = 1, num_packets do
			local track = header.tracks[read_u8(file) + 1]
			if track.kind == 0x00 then
				file:read(read_u16(file) + 1) --command_kind = 1b
			elseif track.kind == 0x01 then
				file:read(track.num_voices * 3)
			end
		end
		file_size = file:seek()
	end

	--tapes hold 6000 bytes per second of play time (computronics' 'tapeLengths' are in minutes)
	local minute = 1500 * 4 * 60
	local default_lengths = {2, 4, 6, 8, 16, 32, 64, 128}
	local function tape_fmt(num_bytes)
		local minutes = num_bytes / minute
		local fits = "none of the default tapes"
		for _, length in ipairs(default_lengths) do
			if length >= minutes then
				fits = ("a %i minute tape"):format(length)
				break
			end
		end
		return ("%i bytes (%.2f minutes), fits on %s"):format(num_bytes, minutes, fits)
	end
	print("tape size needed:")
	print(("  data tape: %s"):format(tape_fmt(file_size)))
	for _, track in ipairs(pcm_tracks) do
		print(("  burner tape for '%s': %s"):format(track.name, tape_fmt(pcm.burner_size(track.num_bytes))))
	end
end

local function play(gpu, file, surfaces)
	local header = read_header(file)
	assertEq(header.magic, szt.magic, "bad magic")
	print("magic: OK")
	assertEq(header.version, szt.version, "bad version")
	print("version: OK")

	if not ops.no_video then
		video.pre_init(gpu)
	end
	local max_size_x, max_size_y = 0, 0
	local max_num_voices = 0
	local num_total_packets = 0
	local has_video, has_audio = false, false
	local pcm_track
	local tracks = {}
	for i, track_desc in ipairs(header.tracks) do
		local track = {
			index = i - 1,
			kind = track_desc.kind,
			num_packets = track_desc.num_packets,
			time_base = track_desc.time_base_num / track_desc.time_base_denom,
			name = track_desc.name,
		}
		num_total_packets = num_total_packets + track.num_packets
		if track_desc.kind == 0x00 then
			has_video = true
			
			local surface = surfaces[track_desc.name] or error(("missing surface for track '%s'"):format(track_desc.name))
			surface.pos_x = surface.is_fullscreen and 1 or surface.pos_x or error("surface has no pos_x")
			surface.pos_y = surface.is_fullscreen and 1 or surface.pos_y or error("surface has no pos_y")

			track.surface = surface
			track.size_x = track_desc.size_x
			track.size_y = track_desc.size_y

			max_size_x = math.max(max_size_x, track.size_x)
			max_size_y = math.max(max_size_y, track.size_y)

			if track.surface.is_fullscreen then
				gpu.bind(track.surface.screen_addr, false)
				gpu.setResolution(track.size_x, track.size_y)
			end
		elseif track_desc.kind == 0x01 then
			has_audio = true

			track.num_voices = track_desc.num_voices
			max_num_voices = math.max(max_num_voices, track.num_voices)
		elseif track_desc.kind == 0x02 and not pcm_track then --a drive plays one block, so the rest are skipped
			track.sample_rate = track_desc.sample_rate
			track.offset = track_desc.offset
			track.num_bytes = track_desc.num_bytes
			track.checksum = track_desc.checksum
			track.duration = track_desc.num_bytes * 8 / track_desc.sample_rate
			pcm_track = track
		end
		table.insert(tracks, track)
	end
	local play_video = has_video and not ops.no_video
	local play_audio = has_audio and not ops.no_audio

	local frames_begin_pos = file:seek()
	local play_pcm = false
	if pcm_track and not ops.no_audio then
		local ok, reason = pcm.prepare(file, pcm_track, frames_begin_pos, num_total_packets > 0)
		if ok then
			play_pcm = true
			print(("pcm audio plays from tape drive %s"):format(reason))
		else
			print(("skipping pcm audio: %s"):format(reason))
			os.sleep(2)
		end
	end
	if play_video then
		local num_buffers = video.init(gpu, max_size_x, max_size_y)
		
		print(("allocated %i buffers!"):format(num_buffers))
		os.sleep(0.1)
	end
	if play_audio then
		audio.init(max_num_voices)
	end

	local function play_impl()
		local video_begin_time_up = computer.uptime()
		local num_live_tracks = 0
		for _, track in ipairs(tracks) do
			track.packet_index = 0
			track.presented_packet_index = 0
			if track.kind == 0x00 and play_video then --video
				track.swap_chain = {}
				num_live_tracks = num_live_tracks + 1
			elseif track.kind == 0x01 and play_audio then --audio
				num_live_tracks = num_live_tracks + 1
			elseif track == pcm_track and play_pcm then
				track.is_live = true
				num_live_tracks = num_live_tracks + 1
			else
				--we probably just wanna skip unknowns rather than get mad about it
				--error("unknown packet kind: %x2", track.kind)
			end
		end

		if play_pcm then
			pcm.start(pcm_track)
		end
		local packet_index = 0
		while num_live_tracks > 0 do
			local frame_begin_time, frame_begin_time_up = os.clock(), computer.uptime()
			if check_interrupted() then
				return false
			end

			local track = nil
			if packet_index < num_total_packets then
				local track_id = read_u8(file)
				track = tracks[track_id + 1]
				if track.kind == 0x00 then --video
					local commands_len = read_u16(file)
					if play_video then
						if commands_len > 0 then
							if ops.no_back then
								if gpu.getScreen() ~= track.surface.screen_addr then
									gpu.bind(track.surface.screen_addr, false)
								end
							else
								video.begin_commit(gpu, track)
							end
							if ops.diff then
								gpu.setBackground(0x000000)
								gpu.setForeground(0xff0000)
								gpu.fill(1, 1, track.size_x, track.size_y, "*")
							end
						elseif not ops.no_back then
							video.begin_dummy_commit(track)
						end

						local command_count = video.draw_track_frame(gpu, file, track, commands_len)
						if ops.fps then
							video.update_stats(track, frame_begin_time, video_begin_time_up, commands_len, command_count)
						end

						if ops.no_back then
							track.presented_packet_index = track.presented_packet_index + 1
							if track.presented_packet_index == track.num_packets then
								num_live_tracks = num_live_tracks - 1
							end
						end
					else
						file:seek("cur", commands_len + 1) --command_kind = 1b
					end
					if ops.no_back then
						--this won't play nice with audio tracks,
						--but we can't really help it without buffers
						if not ops.fast then
							repeat
								local now_up = computer.uptime()
								local current_time = (now_up - video_begin_time_up)
								local next_present_time = track.presented_packet_index * track.time_base
								local time_left = next_present_time - current_time
								os.sleep(time_left) --sleep is no-op when time<=0
							until time_left <= 0
						end
						track.presented_packet_index = track.presented_packet_index + 1
					end
				elseif track.kind == 0x01 then --audio
					if play_audio then
						audio.queue(file, track)
					else
						file:seek("cur", track.num_voices * 3)
					end
				end
			end

			while true do
				local next_present_time = math.huge
				for _, track in ipairs(tracks) do
					if track.kind == 0x00 and play_video and not ops.no_back then
						while #track.swap_chain > 0 do
							local now_up = computer.uptime()
							local current_time = (now_up - video_begin_time_up)
							local presented_packet_time = track.presented_packet_index * track.time_base
							local time_left = presented_packet_time - current_time
							if time_left > 0  and not ops.fast then
								next_present_time = math.min(next_present_time, presented_packet_time)
								break
							end

							if ops.fps then
								video.inject(gpu, track, function() draw_stats(gpu) end)
							end
							video.present(gpu, track)
							track.presented_packet_index = track.presented_packet_index + 1
							if track.presented_packet_index == track.num_packets then
								num_live_tracks = num_live_tracks - 1
							end
						end
					elseif track.kind == 0x01 and play_audio then --audio
						--with --fast the audio runs far ahead of real time, so flush it once the input is done instead of waiting for it to play out
						audio.commit(track, ops.fast and packet_index >= num_total_packets) --commit handles presented_packet_index incrementation
						if ops.fps then
							audio.update_stats(track)
						end
						if track.presented_packet_index == track.num_packets then
							num_live_tracks = num_live_tracks - 1
						end
					elseif track.is_live then --pcm
						if ops.fps then
							pcm.update_stats(track)
						end
						--with --fast the video won't wait for the tape, so stop it with the input
						local is_done = ops.fast and packet_index >= num_total_packets
						if is_done then pcm.stop() end
						if is_done or pcm.update(track, num_live_tracks == 1 and packet_index >= num_total_packets) then
							track.is_live = false
							num_live_tracks = num_live_tracks - 1
						end
					end
				end
				if ops.fps and (play_audio or play_pcm) and not play_video and gpu then
					draw_stats(gpu)
					break
				end
				
				if not play_video or ops.no_back or not video.is_stalled() then break end --only back-buffers can stall

				if next_present_time ~= math.huge and not ops.fast then
					repeat
						local now_up = computer.uptime()
						local current_time = (now_up - video_begin_time_up)
						local time_left = next_present_time - current_time
						os.sleep(time_left) --sleep is no-op when time<=0
					until time_left <= 0
				end
			end

			if track then
				track.packet_index = track.packet_index + 1
			end
			packet_index = packet_index + 1
			--print(("frame took: %05.1fms"):format((computer.uptime() - frame_begin_time_up) * 1000))
		end
		return true
	end

	if ops.loop then
		while play_impl() do
			file:seek("set", frames_begin_pos)
		end
	else
		play_impl()
		if ops.hold then
			while true do
				local e = computer.pullSignal(0)
				if e == "interrupted" or e == "key_down" then break end
			end
		end
	end

	if play_video then
		video.deinit(gpu)
	end
	if play_audio then
		audio.deinit(max_num_voices)
	end
	if play_pcm then
		pcm.deinit()
	end
end

--'burn' writes a szt file to the start of data tapes
local function burn_command()
	if ops.h or ops.help or not args[2] then
		print("burn <file> [drive address...]")
		print("writes a szt file to the start of tapes so '--tape' can play it")
		print("default drive is the primary. give several to fill tapes for 'play_szt_par.lua --tape'")
		print("   --no-label", "don't set the tape label to the file name")
		return
	end

	local path = shell.resolve(args[2])
	local file_size = fs.size(path)

	local drives = {}
	if #args > 2 then
		for i = 3, #args do
			local addr = assert(component.get(args[i], "tape_drive"))
			table.insert(drives, component.proxy(addr))
		end
	else
		local addr = assert(default_tape_drive())
		table.insert(drives, component.proxy(addr))
	end

	for _, drive in ipairs(drives) do
		assert(drive.isReady(), ("no tape in drive %s"):format(drive.address))
		local tape_size = drive.getSize()
		assert(file_size <= tape_size, ("file is %i bytes but the tape in %s only holds %i"):format(file_size, drive.address, tape_size))
	end

	--each drive.write costs a tick, so write big chunks
	local chunk_size = math.floor(math.max(512, math.min(64 * 1024, computer.freeMemory() / 4)))

	for _, drive in ipairs(drives) do
		print(("writing to %s"):format(drive.address))
		drive.stop()
		drive.seek(-drive.getPosition())

		local file = assert(io.open(path, "rb"))
		local written = 0
		while true do
			local chunk = file:read(chunk_size)
			if not chunk then break end
			drive.write(chunk)
			written = written + #chunk
			io.write(("\r%i/%i bytes (%3.f%%)"):format(written, file_size, written / file_size * 100))
		end
		file:close()

		--the drive plays 250ms at a time, so it can overshoot pcm audio played in place at the end. make that silence
		local pad = math.min(3000, drive.getSize() - written)
		if pad > 0 then
			drive.write(string.rep("\x55", pad)) --alternating DFPWM bits hold the level
		end
		print()

		drive.seek(-drive.getPosition())
		if not ops["no-label"] then
			drive.setLabel(fs.name(path))
		end
	end

	print("Done!")
end
if args[1] == "burn" then
	burn_command()
	return
end

if ops.h or ops.help then
	print("-h --help", "show this help")
	print("-v --version", "show player version")
	print("-p --probe", "show the header info")
	print("burn <file> [drive...]", "write a szt file to data tapes. see 'burn --help'")
	print("   --fps", "show performance stats during playback")
	print("   --loop", "loop video like a gif")
	print("   --hold", "hold image on screen")
	print("   --no-video", "skip video tracks")
	print("   --no-audio", "skip audio tracks")
	print("   --no-back", "disable buffering and the dependency on GPU buffers")
	print("   --one-buf", "use only one back buffer to reduce bitblt copies")
	print("   --cfg", "set the screen layout and other environment settings. generate a configs with 'screenConfig.lua'")
	print("   --diff", "only draw what changed from the last frame")
	print("   --fast", "don't wait for frame time; render next frame as fast as possible")
	print("   --batch-check", "debug the batches")
	print("   --pcm-tape", "burner tape drive for pcm audio (default: the first drive the video doesn't read from). it keeps the audio it burned, by checksum, until a new one doesn't fit. it names the tape '" .. burner_label .. "' to find it again")
	print("   --pcm-format", "erase the pcm tape if it isn't blank or a burner tape")
	print("   --tape", "read from tape drives instead of files. the args are drive addresses holding the same data (default: primary). with no args at all, it plays the primary drive. write tapes with 'burn'")
	return
end
if ops.v or ops.version then
	print(("SZT Stream Reader V%s"):format(version))
	print(("understands szt%s containers"):format(szt.version))
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
local path = (#args == 0 and type(ops.tape) == "string") and {ops.tape} or args --'--tape=<addr>' names the drive too
local file, reason = linear_stream.open(path, "rb", ops.tape)
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
pcm.deinit() --don't leave the tape playing after an error

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
