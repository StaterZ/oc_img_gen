local os = require("os")
local io = require("io")
local fs = require("filesystem")
local shell = require("shell")
local unicode = require("unicode")
local component = require("component")
local computer = require("computer")
local serialization = require("serialization")
local event = require("event")

local version = "4.6"

--the player names the scratch tape it burns pcm audio onto, so it can find it again
local scratch_label = "szt pcm burner"

--the primary drive, unless it holds the scratch tape, then another drive
local function default_tape_drive()
	if not component.isAvailable("tape_drive") then
		return nil, "no tape drive found"
	end
	if component.tape_drive.getLabel() == scratch_label then
		for candidate in component.list("tape_drive") do
			local d = component.proxy(candidate)
			if d.isReady() and d.getLabel() ~= scratch_label then
				return candidate
			end
		end
	end
	return component.tape_drive.address
end

--raw stream over a computronics tape drive, the data starts at the beginning of the tape
local tape_stream = {}
do
	local drive_positions = {}

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
		drive.stop()

		drive_positions[addr] = drive.getPosition()

		return setmetatable({
			drive = drive,
			pos = 0,
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
		local addr = self.drive.address
		do
			local drive_pos = drive_positions[addr]
			if self.pos ~= drive_pos then
				self.drive.seek(self.pos - drive_pos)
			end
		end

		local data = self.drive.read(n)
		if not data then return nil, "no tape in drive" end

		self.pos = self.pos + #data
		drive_positions[addr] = self.pos
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
		local stream, reason
		if is_tape then
			stream, reason = tape_stream.open(path)
		else
			path = shell.resolve(path)
			stream, reason = fs.open(path, format)
		end
		if not stream then return nil, reason end

		return setmetatable({
			stream = stream,
			buffer = "",
			bufferHead = 0,
			--each tape read costs a tick, so read more at once
			bufferSize = math.max(512, math.min((is_tape and 32 or 8) * 1024, computer.freeMemory() / 8))
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

		--a skip forward inside the buffer keeps the buffer, so skipping a packet costs no tape read.
		--it returns true instead of the position, since only the stream knows that
		if whence == "cur" and offset > 0 and offset <= #self.buffer - self.bufferHead then
			self.bufferHead = self.bufferHead + offset
			return true
		end

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
	version = 8,
}

local args, ops = shell.parse(...)
local tape_given = ops.tape --a slave only reads a tape when it is told to
ops.tape = ops.tape or #args == 0 --with no file, play the primary tape drive
ops.no_video = ops["no-video"]
ops.no_audio = ops["no-audio"]
ops.no_pcm = ops["no-pcm"]
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
	--all our gpus, the most recently used first
	local gpus = {}
	local main_screen
	function video.init_gpus(primary)
		main_screen = primary.getScreen()
		gpus = {primary}
		for addr in component.list("gpu") do
			if addr ~= primary.address then
				table.insert(gpus, component.proxy(addr))
			end
		end
		return #gpus
	end

	local function move_to_front(idx)
		local gpu = table.remove(gpus, idx)
		table.insert(gpus, 1, gpu)
		return gpu
	end

	--call this instead of gpu.bind. it returns a gpu that is bound to the screen.
	--if no gpu is bound to the screen, it binds the least recently used gpu to it.
	--give a gpu to bind that gpu, because a buffer only exists on the gpu that made it.
	function video.bind(screen_addr, gpu)
		local idx
		for i, candidate in ipairs(gpus) do
			if gpu then
				if candidate.address == gpu.address then
					idx = i
					break
				end
			elseif candidate.getScreen() == screen_addr then
				return move_to_front(i)
			end
		end
		gpu = move_to_front(idx or #gpus)
		if gpu.getScreen() ~= screen_addr then
			gpu.bind(screen_addr, false)
		end
		return gpu
	end

	--each gpu has its own vram, so each gpu has its own pool of free buffers
	local pools = {}

	local function pool_insert(pool, buffer)
		local free = pool.free
		free[#free + 1] = buffer
		pool.free_idx[buffer] = #free
	end

	-- Remove a specific buffer from the pool in O(1). Returns true if the
	-- buffer was in the pool, false if not.
	local function pool_remove(pool, buffer)
		local free, free_idx = pool.free, pool.free_idx
		local idx = free_idx[buffer]
		if not idx then return false end

		local last_idx = #free
		local last_buffer = free[last_idx]
		free[idx] = last_buffer
		free_idx[last_buffer] = idx
		free[last_idx] = nil
		free_idx[buffer] = nil
		return true
	end

	local function pool_pop(pool)
		local free = pool.free
		local last_idx = #free
		local last_buffer = free[last_idx]
		free[last_idx] = nil
		pool.free_idx[last_buffer] = nil
		return last_buffer
	end

	function video.init(size_x, size_y)
		local num_buffers = 0
		if not ops.no_back then
			for _, gpu in ipairs(gpus) do
				local pool = {gpu = gpu, free = {}, free_idx = {}, cache_of = {}}
				pools[gpu.address] = pool
				if size_x > 0 and size_y > 0 then
					repeat
						local buffer = gpu.allocateBuffer(size_x, size_y)
						if not buffer then break end

						pool_insert(pool, buffer)
					until ops.one_buf -- the sallad approves
				end
				if #pool.free == 0 then
					video.deinit()
					error(("can't allocate back-buffer on gpu %s"):format(gpu.address))
				end
				pool.size = #pool.free
				num_buffers = num_buffers + pool.size
			end
		end
		return num_buffers
	end

	function video.deinit()
		for _, pool in pairs(pools) do
			for _, buffer in ipairs(pool.free) do
				pool.gpu.freeBuffer(buffer)
			end
			pool.gpu.setActiveBuffer(0)
		end
		pools = {}
		video.bind(main_screen, gpus[1])
	end

	--after an error, free every buffer and put the primary gpu back on the main screen
	function video.reset(primary)
		for _, gpu in ipairs(gpus) do
			if not ops.no_back then
				gpu.setActiveBuffer(0)
				gpu.freeAllBuffers()
			end
		end
		pools = {}
		if main_screen then
			video.bind(main_screen, primary)
		end
	end

	--the gpu that last bound a screen, for draws that don't care which gpu it is
	function video.any_gpu()
		return gpus[1]
	end

	local function alloc_buffer(pool, cache)
		-- 1. try to reuse the cache
		if pool_remove(pool, cache) then
			return cache
		end

		-- 2. try to find a free buffer (not used for cache)
		for _, candidate in ipairs(pool.free) do
			if not pool.cache_of[candidate] then
				pool_remove(pool, candidate)
				return candidate
			end
		end

		-- 3. worst case, pick anything
		return pool_pop(pool)
	end

	--returns the gpu to draw the frame with
	function video.begin_commit(track)
		--the cache only exists on the gpu that made it, so keep drawing the track with that gpu
		local pool, cache
		if track.cache then
			pool, cache = track.cache.pool, track.cache.buffer
		else --if no cache, create a new back-buffer by copying the screen
			local gpu = video.bind(track.surface.screen_addr)
			pool, cache = pools[gpu.address], 0
		end
		local gpu = pool.gpu

		local buffer = alloc_buffer(pool, cache)
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
		local prev_track = pool.cache_of[buffer]
		if prev_track then
			prev_track.cache = nil
		end
		if track.cache then
			track.cache.pool.cache_of[track.cache.buffer] = nil
		end
		local frame = {pool = pool, buffer = buffer}
		track.cache = frame
		pool.cache_of[buffer] = track
		table.insert(track.swap_chain, frame)
		return gpu
	end

	function video.begin_dummy_commit(track)
		table.insert(track.swap_chain, 0)
	end

	--f gets the gpu to draw with
	function video.inject(track, f)
		local frame = track.swap_chain[1]
		if frame == 0 then --dummy commits have no buffer, so draw on the screen
			f(video.bind(track.surface.screen_addr))
			return
		end

		local gpu = frame.pool.gpu
		local active = gpu.getActiveBuffer()
		gpu.setActiveBuffer(frame.buffer)
		f(gpu)
		gpu.setActiveBuffer(active)
	end

	function video.present(track)
		local frame = table.remove(track.swap_chain, 1)
		if frame == 0 then return end --delete dummy commits

		local gpu = video.bind(track.surface.screen_addr, frame.pool.gpu)
		gpu.bitblt(
			0,
			track.surface.pos_x,
			track.surface.pos_y,
			track.surface.size_x,
			track.surface.size_y,
			frame.buffer
		)
		pool_insert(frame.pool, frame.buffer)
	end

	--the next commit can go to any gpu, so stall when any pool is empty
	function video.is_stalled()
		for _, pool in pairs(pools) do
			if #pool.free == 0 then
				return true
			end
		end
		return false
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
	--data is the command_kind byte and the commands, read at once so they parse with byte() instead of a read per field
	function video.draw_track_frame(gpu, data, track, commands_len)
		local pos_x, pos_y = track.surface.pos_x, track.surface.pos_y

		local command_kind = byte(data, 1)
		local is_braille = command_kind == 0x01 --check 0x01 first since it's likely more common
		if not is_braille and command_kind ~= 0x00 then
			error(("bad frame command kind '%x2'"):format(command_kind))
		end
		local batch_check = ops.batch_check

		local gpu_set, gpu_set_bg, gpu_set_fg = gpu.set, gpu.setBackground, gpu.setForeground

		local command_count = 0
		local i = 2

		local len, x, y, value
		while i <= commands_len + 1 do
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
		play_time,
		commands_len,
		command_count
	)
		local frame_elapsed = os.clock() - frame_begin_time
		local presented_packet_time = track.presented_packet_index * track.time_base
		local packet_header_len = 4 --track_id = 1b, commands_len = 2b, command_kind = 1b
		local packet_len = commands_len + packet_header_len
		local num_free, num_buffers = 0, 0
		for _, pool in pairs(pools) do
			num_free = num_free + #pool.free
			num_buffers = num_buffers + pool.size
		end
		stats[track.index + 1] = ("V|%s: %05i %05i %s %04.1flag %irdy %i/%ipool %04.ffps %05.fms %05ib %04icmds"):format(
			track.name,
			track.packet_index,
			track.presented_packet_index,
			time_fmt(presented_packet_time),
			track.time_base == 0 and 0 or play_time - presented_packet_time,
			#track.swap_chain,
			num_free,
			num_buffers,
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
do --pcm player
	local tape_sample_rate = 48000 --tape drives play 1500 bytes per 250ms at speed 1

	--a scratch tape keeps the blocks it played until a new one doesn't fit after the last one. its layout:
	--  index: "sztp", u8 version, u8 num_entries, then per entry u32 checksum, u32 num_bytes, u32 sample_rate, u32 offset
	--  blocks: from index_size on, in the order they were burned. each one has pad_size of silence after it
	
	local scratch_magic = "sztp"
	local scratch_version = 1
	local index_size = 4096
	local entry_size = 16
	local max_entries = math.min(0xff, math.floor((index_size - 6) / entry_size))
	local pad_size = 3000 --the drive plays up to 250ms at a time, so it can overshoot a block

	local drive
	local block_offset --where the block starts on the drive
	local start_time

	--a drive that the packets are read from can't play at the same time, since a drive has one head
	local function video_drives(files)
		local used = {}
		for _, file in ipairs(files) do
			for _, stream in ipairs(file.streams or {file.stream}) do
				if stream.drive then
					used[stream.drive.address] = true
				end
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
		if data:sub(1, #scratch_magic) ~= scratch_magic then
			if data:find("[^\0]") then return nil end --not blank, so it isn't ours to erase
			return {}
		end
		if data:byte(5) ~= scratch_version then return nil end

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
		local parts = {scratch_magic, string.char(scratch_version, #entries)}
		for _, entry in ipairs(entries) do
			parts[#parts + 1] = string.pack("<I4I4I4I4", entry.checksum, entry.num_bytes, entry.sample_rate, entry.offset)
		end
		seek_to(d, 0)
		d.write(table.concat(parts))
	end

	--copies the block from the szt file onto the scratch tape, unless the tape holds it already. returns where it starts
	local function burn(d, file, track)
		local entries = read_index(d)
		if not entries then
			if not ops.pcm_format then
				return nil, ("the tape in drive %s is not blank or a pcm scratch tape. give --pcm-format to erase it"):format(d.address)
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

		entries[#entries + 1] = {
			checksum = track.checksum,
			num_bytes = track.num_bytes,
			sample_rate = track.sample_rate,
			offset = offset,
		}
		write_index(d, entries)
		return offset
	end

	--picks the drive to play from and gets the block onto it. returns false and why not when it can't.
	--files are the open handles, the first one is used to burn
	function pcm.prepare(files, track, reads_packets)
		local used = video_drives(files)
		local addr
		if type(ops.pcm_tape) == "string" then
			local reason
			addr, reason = component.get(ops.pcm_tape, "tape_drive")
			if not addr then return false, reason end
		else
			--take the tape named as the scratch, else a blank tape, so a drive with other tapes in it is left alone.
			--a tape the video reads from can't be the scratch, even when it has the name
			for candidate in component.list("tape_drive") do
				local d = component.proxy(candidate)
				if not used[candidate] and d.isReady() and d.getLabel() == scratch_label then
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
			--with no packets to read, the drive that holds the file can play the pcm stream in place
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
			local offset, reason = burn(found, files[1], track)
			if not offset then return false, reason end
			block_offset = offset
			if found.getLabel() ~= scratch_label then
				found.setLabel(scratch_label)
			end
		end

		drive = found
		drive.setVolume(math.max(0, math.min(1, tonumber(ops.volume) or 1)))
		return true, addr
	end

	--the bytes a scratch tape needs to hold a block
	function pcm.scratch_size(num_bytes)
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

	local packet_time = 0.25 --the drive sends the client 250ms of audio at a time
	local sent_pos, sent_time, poll_time --the last drive position, the uptime it changed at, the uptime it was read at

	function pcm.start(track)
		drive.stop()
		seek_to(drive, block_offset)
		drive.setSpeed(track.sample_rate / tape_sample_rate)
		drive.play()
		start_time = computer.uptime()
		sent_pos, sent_time, poll_time = block_offset, start_time, nil
	end

	--the play time of the block, from the tape position.
	--the tape does not keep pace with computer.uptime, so the video follows this clock to stay with the audio
	function pcm.time(track)
		local now = computer.uptime()
		if now ~= poll_time then --the position changes once per tick at most, so read it once per tick
			poll_time = now
			local pos = drive.getPosition()
			if pos ~= sent_pos then
				sent_pos, sent_time = pos, now
			end
		end
		local sent_time_s = (sent_pos - block_offset) * 8 / track.sample_rate
		--the client plays the last packet over the next 250ms. then the clock waits for the next packet,
		--unless the block played out and the tape stopped
		local since = now - sent_time
		if sent_time_s < track.duration then
			since = math.min(since, packet_time)
		end
		return math.max(0, sent_time_s - packet_time + since)
	end

	--returns true once the block played out
	function pcm.update(track, is_only_live_track)
		local time_left = track.duration - pcm.time(track)
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
		local elapsed = pcm.time(track)
		stats[track.index + 1] = ("P|%s: %s/%s %iHz x%.2f %s %+05.2fs drift"):format(
			track.name,
			time_fmt(math.min(elapsed, track.duration)),
			time_fmt(track.duration),
			track.sample_rate,
			track.sample_rate / tape_sample_rate,
			drive.getState(),
			elapsed - (computer.uptime() - start_time) --the tape time minus the uptime since it started
		)
	end
end

local function read_header(file)
	local magic = file:read(4)
	local version = read_u16(file)
	local num_tracks = read_u8(file)
	local num_streams = read_u8(file)
	local settings = file:read(read_u16(file))

	local tracks = {}
	for i = 1, num_tracks do
		local track = {
			num_packets = read_u32(file),
			time_base_num = read_u16(file),
			time_base_denom = read_u16(file),
			name = file:read(read_u8(file)),
			stream_id = read_u8(file),
			kind = read_u8(file),
		}

		if track.kind == 0x00 then
			track.size_x = read_u8(file)
			track.size_y = read_u8(file)
		elseif track.kind == 0x01 then
			track.num_voices = read_u8(file)
		elseif track.kind == 0x02 then
			track.sample_rate = read_u32(file)
			track.checksum = read_u32(file)
		end
		table.insert(tracks, track)
	end

	--a stream is the interleaved packets of its tracks, or the DFPWM block of one pcm track
	local streams = {}
	for i = 1, num_streams do
		local stream = {
			index = i - 1,
			offset = read_u32(file),
			num_bytes = read_u32(file),
			tracks = {},
			num_packets = 0,
		}
		table.insert(streams, stream)
	end

	for _, track in ipairs(tracks) do
		local stream = streams[track.stream_id + 1] or error(("track '%s' is on stream %i, but there are only %i"):format(track.name, track.stream_id, num_streams))
		table.insert(stream.tracks, track)
		stream.num_packets = stream.num_packets + track.num_packets
		if track.kind == 0x02 then
			--the pcm track is the whole stream
			stream.is_pcm = true
			track.offset = stream.offset
			track.num_bytes = stream.num_bytes
		end
	end

	return {
		magic = magic,
		version = version,
		settings = settings,
		tracks = tracks,
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

		print(("  %i: %s | '%s' | stream %i"):format(i - 1, kind_text, track.name, track.stream_id))
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

	print(("found %i streams:"):format(#header.streams))
	for _, stream in ipairs(header.streams) do
		print(("  %i: %s | %i bytes at byte %i | %i tracks"):format(
			stream.index,
			stream.is_pcm and "pcm" or ("%i packets"):format(stream.num_packets),
			stream.num_bytes,
			stream.offset,
			#stream.tracks
		))
	end

	local headers_end = file:seek()
	print(("headers done at byte: %i"):format(headers_end))

	--the streams say where they end, so the file ends after the last one
	local file_size = headers_end
	local pcm_tracks = {}
	for _, stream in ipairs(header.streams) do
		file_size = math.max(file_size, stream.offset + stream.num_bytes)
	end
	for _, track in ipairs(header.tracks) do
		if track.kind == 0x02 then
			table.insert(pcm_tracks, track)
		end
	end

	--tapes hold 6000 bytes per second of play time (computronics' 'tapeLengths' are in minutes)
	local minute = 1500 * 4 * 60
	local default_lengths = {2, 4, 6, 8, 16, 32, 64, 128}
	local function tape_fmt(num_bytes)
		local minutes = num_bytes / minute
		local fits = "no tape"
		for _, length in ipairs(default_lengths) do
			if length >= minutes then
				fits = ("a %i minute tape"):format(length)
				break
			end
		end
		return ("%ib / %.2fm, fits on %s"):format(num_bytes, minutes, fits)
	end
	print("tape size needed:")
	print(("  data tape: %s"):format(tape_fmt(file_size)))
	for _, track in ipairs(pcm_tracks) do
		print(("  scratch tape for '%s': %s"):format(track.name, tape_fmt(pcm.scratch_size(track.num_bytes))))
	end
end

--a master player plays some video tracks on the screens of slave players, in sync.
--  only the master has a --cfg. a slave answers "hello" with the addresses of its screens.
--  the master gives each slave the video tracks whose surfaces are on its screens.
--  a slave plays the szt file from its own tape. at setup the master sends it the file, unless its tape has the file already.
--  the master broadcasts its clock each second. there is no ping: a slave that the master found stays in the session.
--  a sample can only come late, never early, so the slave follows the sample that is the most ahead.
--  messages: tag "szt", then the kind and its values. a pass starts with the first clock of a new gen, and ends with "end" or "stop"
local net = {}
do
	net.default_port = 5741
	local discover_min, discover_quiet, discover_max = 1, 1.5, 30 --seconds the master waits for the claims of the slaves
	net.preroll = 0.25 --without pcm, the master starts its clock this much later, so the first clock reaches the slaves first
	net.default_label = "szt video" --the tape label when the master plays an unlabeled tape
	--the options of the master that the slaves play with. --fast and --loop are not here: the clock of the master paces the slaves
	net.shared_ops = {"fps", "hold", "no_video", "diff", "no_back", "one_buf", "batch_check"}
	local clock_interval = 1 --the master sends its clock this often, so the slaves follow a pcm tape too
	local latency = 0.05 --a message takes at least one tick
	local num_samples = 3 --a slave follows the best of the last clock samples
	local timeout = 30 --a slave drops a master that is quiet this long, so a crashed master does not hold it forever
	local resend_after = 3 --the master sends the setup again to a slave that did not answer this long
	local chunk_timeout = 2 --a slave asks again for a chunk of the file that did not come in this long
	local window = 4 --a slave asks for this many chunks of the file at once
	local tag = "szt"

	function net.port(value)
		if value == true then return net.default_port end
		local port = tonumber(value)
		if not port or port < 1 or port > 65535 or port % 1 ~= 0 then
			error(("bad port '%s'"):format(tostring(value)), 0)
		end
		return port
	end

	local function open_modem(port)
		if not component.isAvailable("modem") then
			error("--slave and --master need a network card (a linked card has no ports, so it does not work)", 0)
		end
		local modem = component.modem
		local was_open = modem.isOpen(port)
		if not was_open and not modem.open(port) then
			error(("can't open port %i"):format(port), 0)
		end
		return modem, was_open
	end

	--slave errors that stop a session are tables, so they don't print a traceback
	local function fail(msg)
		error({net = msg}, 0)
	end
	function net.error_text(err)
		if type(err) == "table" and err.net then return err.net end
	end

	--the streams say where they end, so the file ends after the last one
	local function file_size(header)
		local size = 0
		for _, stream in ipairs(header.streams) do
			size = math.max(size, stream.offset + stream.num_bytes)
		end
		return size
	end

	--two players with the same signature have the same file
	function net.header_sig(header)
		local parts = {header.magic, tostring(header.version), header.settings}
		for _, track in ipairs(header.tracks) do
			table.insert(parts, ("%s/%i/%i/%i/%i/%s"):format(track.name, track.kind, track.num_packets,
				track.time_base_num, track.time_base_denom, tostring(track.checksum)))
		end
		for _, stream in ipairs(header.streams) do
			table.insert(parts, ("%i/%i"):format(stream.offset, stream.num_bytes))
		end
		return table.concat(parts, ";")
	end

	local master = {}
	master.__index = master

	function net.master(port)
		local modem, was_open = open_modem(port)
		local self = setmetatable({
			modem = modem,
			port = port,
			was_open = was_open,
			--a chunk of the file is the message size minus room for the tag, kind and offset
			chunk_size = (modem.maxPacketSize and modem.maxPacketSize() or 8192) - 64,
			slaves = {}, --by address
			on_slave = {}, --by track index (from 1), true when a slave plays the track
			wants = {}, --the chunks of the file that slaves asked for
			gen = 0,
		}, master)
		self.listener = function(_, _, from, msg_port, _, msg_tag, kind, a)
			if msg_port == port and msg_tag == tag then
				self:receive(from, kind, a)
			end
		end
		event.listen("modem_message", self.listener)
		return self
	end

	function master:receive(from, kind, a)
		local slave = self.slaves[from]
		if slave then
			slave.heard = computer.uptime()
		end
		if kind == "claim" and self.claims then
			self.claims[from] = serialization.unserialize(a)
		elseif kind == "want" then
			local slave = self.slaves[from]
			if slave and slave.state == "setup" then
				table.insert(self.wants, {slave = slave, offset = a})
			end
		elseif kind == "ready" or kind == "fail" then
			local slave = self.slaves[from]
			if slave and slave.state == "setup" then
				slave.state = kind
				slave.reason = a
			end
		end
	end

	--asks the slaves on the port which screens they have.
	--a video track whose surface is on a screen of a slave goes to that slave, with its surface
	function master:discover(header, surfaces)
		print(("looking for slaves on port %i"):format(self.port))
		self.claims = {}
		self.modem.broadcast(self.port, tag, "hello")
		--the listener collects the claims. a relay forwards only a few messages each second,
		--so the claims of many slaves come in one by one. wait until no new claim comes for a while
		local begin = computer.uptime()
		local last_new, num_claims = begin, 0
		while true do
			os.sleep(0.25)
			local count = 0
			for _ in pairs(self.claims) do count = count + 1 end
			local t = computer.uptime()
			if count > num_claims then
				num_claims, last_new = count, t
			end
			if t - begin >= discover_min and t - last_new >= discover_quiet then break end
			if t - begin >= discover_max then break end
		end
		local claims = self.claims
		self.claims = nil

		local owner = {} --screen address -> slave
		local found = {}
		for addr, claim in pairs(claims) do
			local slave = {address = addr, tracks = {}}
			for _, screen in ipairs(type(claim) == "table" and claim.screens or {}) do
				owner[screen] = slave
			end
			table.insert(found, addr:sub(1, 8))
		end
		print(("found %i slaves %s"):format(#found, table.concat(found, " ")))

		for i, desc in ipairs(header.tracks) do
			local surface = surfaces[desc.name]
			local screen = surface and surface.screen_addr
			--a screen of this computer plays here
			if desc.kind == 0x00 and screen and select(2, pcall(component.type, screen)) ~= "screen" then
				local slave = owner[screen]
				if not slave then
					error(("no computer has the screen %s of track '%s'. start its slave first"):format(screen, desc.name), 0)
				end
				table.insert(slave.tracks, {
					name = desc.name,
					surface = {
						screen_addr = surface.screen_addr,
						is_fullscreen = surface.is_fullscreen,
						pos_x = surface.pos_x,
						pos_y = surface.pos_y,
					},
				})
				self.on_slave[i] = true
				self.slaves[slave.address] = slave
			end
		end

		for addr, slave in pairs(self.slaves) do
			local names = {}
			for _, track in ipairs(slave.tracks) do
				table.insert(names, track.name)
			end
			print(("slave %s plays %s"):format(addr:sub(1, 8), table.concat(names, ", ")))
		end
	end

	--true when a slave plays the track
	function master:plays_on_slave(track_index)
		return self.on_slave[track_index] == true
	end

	--sends each slave its tracks, then sends the file to the slaves that ask for it.
	--waits until the slaves have the file on tape, synced and allocated their buffers
	function master:setup(header, open_file)
		local size = file_size(header)
		for addr, slave in pairs(self.slaves) do
			local text = serialization.serialize({
				sig = net.header_sig(header),
				source = self.source, --where the slave reads the file: {path=...} or {label=...}, like the master
				ops = self.ops, --the playback options of the master
				size = size,
				chunk_size = self.chunk_size,
				tracks = slave.tracks,
			})
			if #text > self.chunk_size then
				error(("the setup for slave %s is %i bytes, but a message holds %i"):format(addr, #text, self.chunk_size), 0)
			end
			slave.state = "setup"
			slave.sent = 0
			slave.text = text
			slave.heard = computer.uptime()
			self.modem.send(addr, self.port, tag, "setup", text)
		end

		--a slave that the master found stays in. the master waits for each one until it is ready or fails.
		--a lost message only makes it wait longer: the setup goes out again, and the slave asks again for a chunk
		local handle --the file again, so the chunks don't move the play position
		local next_print = 0
		local next_wait_print = computer.uptime() + 5
		while true do
			if check_interrupted() then fail("interrupted") end
			local waiting = {}
			for addr, slave in pairs(self.slaves) do
				if slave.state == "setup" then
					table.insert(waiting, addr:sub(1, 8))
					if computer.uptime() - slave.heard >= resend_after then
						slave.heard = computer.uptime()
						self.modem.send(addr, self.port, tag, "setup", slave.text)
					end
				end
			end
			if #waiting == 0 then break end
			if computer.uptime() >= next_wait_print then
				next_wait_print = computer.uptime() + 5
				print(("waiting for slaves %s (ctrl+c stops)"):format(table.concat(waiting, " ")))
			end

			local want = table.remove(self.wants, 1)
			if want then
				if not handle then
					local reason
					handle, reason = open_file()
					if not handle then error(("can't open the file again to send it: %s"):format(reason), 0) end
				end
				local n = math.min(self.chunk_size, size - want.offset)
				if n > 0 then
					handle:seek("set", want.offset)
					self.modem.send(want.slave.address, self.port, tag, "chunk", want.offset, handle:read(n))
					want.slave.sent = math.max(want.slave.sent, want.offset + n)
				end
				next_wait_print = computer.uptime() + 5
				if computer.uptime() >= next_print then
					next_print = computer.uptime() + 1
					for addr, slave in pairs(self.slaves) do
						if slave.sent > 0 and slave.state == "setup" then
							print(("sending the file to slave %s: %i/%i bytes (%3.f%%)"):format(addr:sub(1, 8), slave.sent, size, slave.sent / size * 100))
						end
					end
				end
			else
				os.sleep(0.05)
			end
		end
		if handle then
			handle:close()
		end
		self.wants = {}

		for addr, slave in pairs(self.slaves) do
			slave.text = nil
			if slave.state ~= "ready" then
				print(("dropping slave %s: %s"):format(addr:sub(1, 8), slave.reason or "it failed"))
				self.slaves[addr] = nil
			end
		end
		self.active = next(self.slaves) ~= nil
	end

	--clock gives the play time of the pass
	function master:begin_pass(clock)
		if not self.active then return end
		self.gen = self.gen + 1
		self.clock = clock
		self:send_clock() --the first clock of a new gen starts the pass on the slaves
	end

	function master:send_clock()
		local play_time = self.clock()
		self.clock_sent = computer.uptime()
		self.modem.broadcast(self.port, tag, "clock", self.gen, play_time)
	end

	--call this often during a pass
	function master:tick()
		if not self.active then return end
		if computer.uptime() - self.clock_sent >= clock_interval then
			self:send_clock()
		end
	end

	function master:stats_line()
		local n = 0
		for _ in pairs(self.slaves) do n = n + 1 end
		return ("N|master: %i slaves, clock sent %.1fs ago"):format(n, self.clock_sent and computer.uptime() - self.clock_sent or -1)
	end

	--completed is false when the player was interrupted
	function master:finish(completed)
		if not self.active then return end
		--twice, so one lost message does not leave the slaves waiting
		for _ = 1, 2 do
			self.modem.broadcast(self.port, tag, completed and "end" or "stop", self.gen)
		end
		self.finished = true
	end

	function master:close()
		if self.active and not self.finished then
			for _ = 1, 2 do
				self.modem.broadcast(self.port, tag, "stop", self.gen)
			end
		end
		event.ignore("modem_message", self.listener)
		if not self.was_open then
			self.modem.close(self.port)
		end
	end

	local slave = {}
	slave.__index = slave

	--the master sends the surfaces, so a slave needs no --cfg.
	--tape is the --tape option: a drive address, true for the primary drive, or nil to find the tape
	function net.slave(port, tape)
		local modem, was_open = open_modem(port)
		local self = setmetatable({
			is_slave = true,
			modem = modem,
			port = port,
			was_open = was_open,
			tape = tape,
		}, slave)
		self:end_session()
		self.listener = function(_, _, from, msg_port, _, msg_tag, kind, a, b, c)
			if msg_port == port and msg_tag == tag then
				self:receive(from, kind, a, b, c)
			end
		end
		self.on_interrupt = function()
			self.quit = true
		end
		event.listen("modem_message", self.listener)
		event.listen("interrupted", self.on_interrupt)
		return self
	end

	function slave:end_session()
		self.master = nil
		self.queue, self.head, self.tail = {}, 1, 0
		self.chunks = {}
		self.clocks = {}
		self.gen = 0
		self.stopped, self.ended, self.readied, self.failed = false, false, false, false
	end

	function slave:push(msg)
		self.tail = self.tail + 1
		self.queue[self.tail] = msg
	end

	function slave:pop()
		local msg = self.queue[self.head]
		if msg then
			self.queue[self.head] = nil
			self.head = self.head + 1
		end
		return msg
	end

	function slave:receive(from, kind, a, b, c)
		if not self.master then
			if kind == "hello" then
				local screens = {}
				for addr in component.list("screen") do
					table.insert(screens, addr)
				end
				self.modem.send(from, self.port, tag, "claim", serialization.serialize({screens = screens}))
			elseif kind == "setup" then
				self.master = from
				self:push({kind = kind, data = a})
			end
			return
		end
		if from ~= self.master then return end --a slave serves one master at a time

		local now = computer.uptime()
		self.heard = now
		if kind == "clock" then
			--the play time minus the uptime it came in at. a late sample gives a lower value, so take the highest
			local entry = self.clocks[a] or {bases = {}}
			self.clocks[a] = entry
			table.insert(entry.bases, b - now)
			if #entry.bases > num_samples then
				table.remove(entry.bases, 1)
			end
			entry.base = math.max(table.unpack(entry.bases))
		elseif kind == "chunk" then
			self.chunks[a] = b
		elseif kind == "stop" then
			self.stopped = true
		elseif kind == "end" then
			self.ended = true
		elseif kind == "setup" then
			--the master did not hear the answer, so send it again
			if self.readied then
				self.modem.send(self.master, self.port, tag, "ready")
			end
		end
	end

	--raises an error when the session ends
	function slave:check()
		if self.quit then fail("interrupted") end
		if self.stopped then fail("the master stopped") end
		local now = computer.uptime()
		local quiet = now - self.heard
		if quiet > timeout and not self.ended then
			fail(("the master is quiet for %.0fs"):format(quiet))
		end
	end

	--returns the setup from the master, or nil when interrupted: sig, size, chunk_size, and tracks with the name and surface of each
	function slave:wait_setup()
		print(("waiting for a master on port %i"):format(self.port))
		while not self.quit do
			local msg = self:pop()
			if msg and msg.kind == "setup" then
				self.heard = computer.uptime()
				local setup = serialization.unserialize(msg.data)
				setup.surfaces = {}
				for _, track in ipairs(setup.tracks) do
					setup.surfaces[track.name] = track.surface
				end
				return setup
			end
			os.sleep(0.25)
		end
	end

	--the signature of the szt file at the source, or nil
	local function source_sig(src, is_tape)
		if not is_tape and not fs.exists(src) then return nil end
		local file = linear_stream.open(src, "rb", is_tape)
		if not file then return nil end
		local ok, sig = pcall(function()
			if file:read(4) ~= szt.magic then return nil end
			file:seek("set", 0)
			return net.header_sig(read_header(file))
		end)
		file:close()
		return ok and sig or nil
	end

	local function is_blank(drive)
		drive.seek(-drive.getPosition())
		local data = drive.read(4096)
		drive.seek(-drive.getPosition())
		return data ~= nil and not data:find("[^\0]")
	end

	--the drive for the file: the one --tape names, else the tape with the label, else a blank tape
	local function pick_drive(self, size, label)
		local addr
		if not component.isAvailable("tape_drive") then
			fail("this computer has no tape drive. the master plays a tape, so the slave needs one too")
		end
		if self.tape == true then
			addr = default_tape_drive()
		elseif self.tape then
			addr = component.get(self.tape, "tape_drive")
		else
			for candidate in component.list("tape_drive") do
				local d = component.proxy(candidate)
				if d.isReady() and d.getLabel() == label then
					addr = candidate
					break
				end
			end
			if not addr then
				for candidate in component.list("tape_drive") do
					local d = component.proxy(candidate)
					if d.isReady() then
						d.stop()
						if is_blank(d) then
							addr = candidate
							break
						end
					end
				end
			end
		end
		if not addr then
			fail(("no tape for the video. put the tape '%s' or a blank tape in a tape drive, or give --tape=<drive> to write over a tape"):format(label))
		end
		local drive = component.proxy(addr)
		if not drive.isReady() then
			fail(("no tape in drive %s"):format(addr))
		end
		if drive.getSize() < size then
			fail(("the video is %i bytes, but the tape in drive %s only holds %i"):format(size, addr, drive.getSize()))
		end
		return drive
	end

	--gets the file from the master in chunks and gives them to write in order
	local function copy(self, setup, write)
		local size, chunk_size = setup.size, setup.chunk_size
		local written, next_want = 0, 0
		local asked_at = computer.uptime()
		local next_print = 0
		self.chunks = {}
		while written < size do
			self:check()
			--ask for the next chunks, at most window ahead of the write position
			while next_want < size and next_want < written + window * chunk_size do
				self.modem.send(self.master, self.port, tag, "want", next_want)
				next_want = next_want + chunk_size
				asked_at = computer.uptime()
			end
			--write the chunks that are next. each tape write costs a tick, so write them together
			local parts = {}
			while self.chunks[written] do
				local data = self.chunks[written]
				self.chunks[written] = nil
				table.insert(parts, data)
				written = written + #data
			end
			if #parts > 0 then
				write(table.concat(parts))
				asked_at = computer.uptime()
				if computer.uptime() >= next_print then
					next_print = computer.uptime() + 1
					io.write(("\r%i/%i bytes (%3.f%%)"):format(written, size, written / size * 100))
				end
			elseif computer.uptime() - asked_at > chunk_timeout then
				next_want = written --a chunk got lost, so ask again from the write position
			else
				os.sleep(0.05)
			end
		end
		self.chunks = {}
		print()
	end

	--makes sure the slave has the file of the master at the same place as the master: the same path, or a tape with the same label.
	--copies it from the master when not. --tape on the slave puts it on that drive instead. returns the source and if it is a tape
	function slave:fetch(setup)
		local source = setup.source or {label = net.default_label}
		if source.label or self.tape then
			local label = source.label or fs.name(source.path)
			local drive = pick_drive(self, setup.size, label)
			drive.stop()
			if source_sig(drive.address, true) == setup.sig then
				print(("tape drive %s has the video already"):format(drive.address))
			else
				print(("copying the video from the master onto tape drive %s"):format(drive.address))
				drive.seek(-drive.getPosition())
				copy(self, setup, function(data) drive.write(data) end)
				drive.seek(-drive.getPosition())
				if drive.getLabel() ~= label then
					drive.setLabel(label)
				end
				if source_sig(drive.address, true) ~= setup.sig then
					fail(("the copy on tape drive %s does not match the file of the master"):format(drive.address))
				end
			end
			return drive.address, true
		end

		local path = source.path
		if source_sig(path, false) == setup.sig then
			print(("%s has the video already"):format(path))
			return path, false
		end
		print(("copying the video from the master to %s"):format(path))
		local dir = fs.path(path)
		if dir and dir ~= "" and not fs.exists(dir) then
			fs.makeDirectory(dir)
		end
		local handle, reason = fs.open(path, "wb")
		if not handle then
			fail(("cannot write %s: %s"):format(path, tostring(reason)))
		end
		local ok, err = pcall(copy, self, setup, function(data)
			local done, why = handle:write(data)
			if not done then
				fail(("cannot write %s: %s"):format(path, tostring(why)))
			end
		end)
		handle:close()
		if not ok then error(err, 0) end
		if source_sig(path, false) ~= setup.sig then
			fail(("the copy at %s does not match the file of the master"):format(path))
		end
		return path, false
	end
	function slave:ready()
		self.readied = true
		self.modem.send(self.master, self.port, tag, "ready")
	end

	--tells the master when the setup failed
	function slave:report(err)
		if self.master and not self.readied then
			self.modem.send(self.master, self.port, tag, "fail", tostring(err):sub(1, 200))
		end
	end

	--waits for the master to start a pass: the first clock of a new gen. returns false when the master is done
	function slave:begin_pass()
		while true do
			self:check()
			local newest
			for gen in pairs(self.clocks) do
				if gen > self.gen and (not newest or gen > newest) then
					newest = gen
				end
			end
			if newest then
				self.gen = newest
				for gen in pairs(self.clocks) do
					if gen < self.gen then
						self.clocks[gen] = nil
					end
				end
				return true
			end
			if self.ended then
				return false
			end
			os.sleep(0.05)
		end
	end

	--the play time of the master, from the best of its latest samples
	function slave:clock()
		self:check()
		return self.clocks[self.gen].base + computer.uptime() + latency
	end

	function slave:stats_line()
		return ("N|slave: last heard from the master %.1fs ago"):format(computer.uptime() - self.heard)
	end

	function slave:close()
		event.ignore("modem_message", self.listener)
		event.ignore("interrupted", self.on_interrupt)
		if not self.was_open then
			self.modem.close(self.port)
		end
	end
end

--open_file opens the file again, for streams read next to the first one.
--link is the master or slave link of --master and --slave. a slave gives its header, and is the file too
local function play(gpu, file, surfaces, open_file, link, header)
	header = header or read_header(file)
	assertEq(header.magic, szt.magic, "bad magic")
	print("magic: OK")
	assertEq(header.version, szt.version, "bad version")
	print("version: OK")

	local master_link = link and not link.is_slave and link or nil
	local slave_link = link and link.is_slave and link or nil
	if master_link then
		master_link:discover(header, surfaces)
	end

	local max_size_x, max_size_y = 0, 0
	local max_num_voices = 0
	local has_video, has_audio = false, false
	local pcm_track
	local tracks = {}
	local track_of = {} --by track_desc
	for i, track_desc in ipairs(header.tracks) do
		local track = {
			index = i - 1,
			kind = track_desc.kind,
			num_packets = track_desc.num_packets,
			time_base = track_desc.time_base_num / track_desc.time_base_denom,
			name = track_desc.name,
		}
		if track_desc.kind == 0x00 then
			--a track plays here, or on the slave that has its screen
			track.on_slave = master_link ~= nil and master_link:plays_on_slave(i)
			local surface = not track.on_slave and surfaces[track_desc.name] or nil
			--a slave with the file plays only the tracks that the master gave it
			if not surface and not track.on_slave and not slave_link then
				error(("missing surface for track '%s'"):format(track_desc.name))
			end
			--a screen of another computer can't be drawn here, so skip the track
			if surface and surface.screen_addr and select(2, pcall(component.type, surface.screen_addr)) ~= "screen" then
				print(("skipping track '%s': screen %s is not on this computer"):format(track_desc.name, surface.screen_addr))
				surface = nil
			end
			track.size_x = track_desc.size_x
			track.size_y = track_desc.size_y

			if surface then
				has_video = true
				surface.pos_x = surface.is_fullscreen and 1 or surface.pos_x or error("surface has no pos_x")
				surface.pos_y = surface.is_fullscreen and 1 or surface.pos_y or error("surface has no pos_y")
				track.surface = surface

				max_size_x = math.max(max_size_x, track.size_x)
				max_size_y = math.max(max_size_y, track.size_y)

				if track.surface.is_fullscreen then
					video.bind(track.surface.screen_addr).setResolution(track.size_x, track.size_y)
				end
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
		track_of[track_desc] = track
	end
	local play_video = has_video and not ops.no_video
	local play_audio = has_audio and not ops.no_audio and not slave_link --the master plays the audio
	local play_pcm   = pcm_track and not ops.no_pcm and not slave_link
	for _, track in ipairs(tracks) do
		track.is_shown = play_video and track.surface ~= nil
	end

	--a stream is read when it holds a played track. it gets its own handle, so streams don't seek each other away.
	--the pcm stream isn't read, a tape drive plays it
	local packet_streams = {}
	local num_total_packets = 0
	for _, stream_desc in ipairs(header.streams) do
		local is_read = false
		for _, track_desc in ipairs(stream_desc.tracks) do
			local track = track_of[track_desc]
			if track.is_shown or (track_desc.kind == 0x01 and play_audio) then
				is_read = true
			end
		end
		if is_read and not stream_desc.is_pcm and stream_desc.num_packets > 0 then
			table.insert(packet_streams, {
				index = stream_desc.index,
				offset = stream_desc.offset,
				num_packets = stream_desc.num_packets,
			})
			num_total_packets = num_total_packets + stream_desc.num_packets
		end
	end
	local files = {file}
	for i, stream in ipairs(packet_streams) do
		if i == 1 then
			stream.file = file
		else
			local reason
			stream.file, reason = open_file()
			if not stream.file then error(("can't open the file again for stream %i: %s"):format(stream.index, reason)) end
			table.insert(files, stream.file)
		end
	end

	if play_pcm then
		local ok, reason = pcm.prepare(files, pcm_track, num_total_packets > 0)
		if ok then
			print(("pcm audio plays from tape drive %s"):format(reason))
		else
			play_pcm = false
			print(("skipping pcm audio: %s"):format(reason))
			os.sleep(2)
		end
	end
	if play_video then
		local num_buffers = video.init(max_size_x, max_size_y)
		
		print(("allocated %i buffers!"):format(num_buffers))
		os.sleep(0.1)
	end
	if play_audio then
		audio.init(max_num_voices)
	end
	--the slaves get ready after this player, so a long pcm burn can't make them give up
	if master_link then
		master_link:setup(header, open_file)
	elseif slave_link then
		slave_link:ready()
	end

	local function play_impl()
		--a slave plays each pass that the master starts
		if slave_link and not slave_link:begin_pass() then
			return false
		end
		for _, stream in ipairs(packet_streams) do
			stream.file:seek("set", stream.offset)
			stream.packet_index = 0
			stream.next_track = nil
		end

		local video_begin_time_up = computer.uptime()
		if master_link and master_link.active then
			--give the first packets time to reach the slaves before they play
			video_begin_time_up = video_begin_time_up + net.preroll
		end
		--with pcm audio the tape is the master clock. a slave follows the clock of its master
		local function play_clock()
			if slave_link then
				return slave_link:clock()
			end
			if play_pcm then
				return pcm.time(pcm_track)
			end
			return computer.uptime() - video_begin_time_up
		end
		local num_live_tracks = 0
		local num_slave_live = 0
		for _, track in ipairs(tracks) do
			track.packet_index = 0
			track.presented_packet_index = 0
			track.is_slave_live = false
			if track.is_shown then --video
				track.swap_chain = {}
				num_live_tracks = num_live_tracks + 1
			elseif track.on_slave and master_link.active then --live until it played out on the slave, so the clock goes on
				track.is_slave_live = true
				num_live_tracks = num_live_tracks + 1
				num_slave_live = num_slave_live + 1
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
		if master_link then
			master_link:begin_pass(play_clock)
		end

		local function draw_frame(track, commands_len, data)
			local draw_begin_time = os.clock()
			local draw_gpu
			if commands_len > 0 then
				if ops.no_back then
					draw_gpu = video.bind(track.surface.screen_addr)
				else
					draw_gpu = video.begin_commit(track)
				end
				if ops.diff then
					draw_gpu.setBackground(0x000000)
					draw_gpu.setForeground(0xff0000)
					draw_gpu.fill(1, 1, track.size_x, track.size_y, "*")
				end
			else
				if not ops.no_back then
					video.begin_dummy_commit(track)
				end
				draw_gpu = video.any_gpu() --an empty frame draws nothing
			end

			local command_count = video.draw_track_frame(draw_gpu, data, track, commands_len)
			if ops.fps then
				video.update_stats(track, draw_begin_time, play_clock(), commands_len, command_count)
			end
		end

		local packet_index = 0
		while num_live_tracks > 0 do
			local frame_begin_time, frame_begin_time_up = os.clock(), computer.uptime()
			if check_interrupted() then
				if slave_link then
					slave_link.quit = true
				end
				return false
			end

			local track = nil
			if packet_index < num_total_packets then
				--each stream is in play order already, so take the packet that ends first, like the encoder did
				local stream, best_end_time
				for _, candidate in ipairs(packet_streams) do
					if candidate.packet_index < candidate.num_packets then
						if not candidate.next_track then
							candidate.next_track = tracks[read_u8(candidate.file) + 1]
						end
						local end_time = (candidate.next_track.packet_index + 1) * candidate.next_track.time_base
						if not best_end_time or end_time < best_end_time then
							stream, best_end_time = candidate, end_time
						end
					end
				end
				track = stream.next_track
				stream.next_track = nil
				stream.packet_index = stream.packet_index + 1

				local file = stream.file
				if track.kind == 0x00 then --video
					local commands_len = read_u16(file)
					if track.is_shown then
						draw_frame(track, commands_len, file:read(commands_len + 1)) --command_kind = 1b
					else
						file:seek("cur", commands_len + 1)
					end
					if ops.no_back then
						track.presented_packet_index = track.presented_packet_index + 1
						if track.is_shown and track.presented_packet_index == track.num_packets then
							num_live_tracks = num_live_tracks - 1
						end

						--this won't play nice with audio tracks,
						--but we can't really help it without buffers
						if not ops.fast then
							repeat
								local current_time = play_clock()
								local next_present_time = track.presented_packet_index * track.time_base
								local time_left = next_present_time - current_time
								os.sleep(time_left) --sleep is no-op when time<=0
							until time_left <= 0
						end
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
					if track.is_shown and not ops.no_back then
						while #track.swap_chain > 0 do
							local current_time = play_clock()
							local presented_packet_time = track.presented_packet_index * track.time_base
							local time_left = presented_packet_time - current_time
							if time_left > 0  and not ops.fast then
								next_present_time = math.min(next_present_time, presented_packet_time)
								break
							end

							if ops.fps then
								video.inject(track, draw_stats)
							end
							video.present(track)
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
					elseif track.is_slave_live then
						local time_left = track.num_packets * track.time_base - play_clock()
						if time_left <= 0 then
							track.is_slave_live = false
							num_live_tracks = num_live_tracks - 1
							num_slave_live = num_slave_live - 1
						elseif num_live_tracks == num_slave_live and packet_index + 1 >= num_total_packets and not ops.fast then
							os.sleep(math.min(time_left, 0.5)) --nothing else to do here, so wait but still send the clock
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
				if master_link then
					master_link:tick()
				end
				if ops.fps and link then
					stats[#tracks + 1] = link:stats_line()
				end
				if ops.fps and (play_audio or play_pcm) and not play_video and gpu then
					draw_stats(gpu)
					break
				end

				local is_stalled = play_video and not ops.no_back and video.is_stalled() --only back-buffers can stall
				if not is_stalled then break end
				if not is_stalled and not is_ahead then break end

				local wake_time = next_present_time
				if wake_time ~= math.huge and not ops.fast then
					repeat
						local current_time = play_clock()
						local time_left = wake_time - current_time
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

	if slave_link then
		while play_impl() do end
	elseif ops.loop then
		while play_impl() do end
		if master_link then
			master_link:finish(false) --a loop only ends when interrupted
		end
	else
		local completed = play_impl()
		if master_link then
			master_link:finish(completed)
		end
		if ops.hold then
			while true do
				local e = computer.pullSignal(0)
				if e == "interrupted" or e == "key_down" then break end
			end
		end
	end

	if play_video then
		video.deinit()
	end
	if play_audio then
		audio.deinit(max_num_voices)
	end
	if play_pcm then
		pcm.deinit()
	end
	for i = 2, #files do
		files[i]:close()
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
	print("   --pcm-tape", "scratch tape drive for pcm audio (default: the first drive the video doesn't read from). it keeps the audio it burned, by checksum, until a new one doesn't fit. it names the tape '" .. scratch_label .. "' to find it again")
	print("   --pcm-format", "erase the pcm tape if it isn't blank or a scratch tape")
	print("   --tape", "read from a tape drive instead of a file. the arg is the drive address (default: primary). with no args at all, it plays the primary drive. write tapes with 'burn'")
	print("   --slave[=port]", ("wait for a master on a modem port (default: %i) and play the video tracks that it gives this computer. a slave needs no --cfg, it tells the master its screens. the slave reads the file from the same place as the master: the same path, or a tape with the same label. when it does not have the file there, the master copies it there first (onto the labeled tape, else a blank tape). --tape=<drive> on the slave puts the copy on that drive. the master also sets --fps, --hold, --no-video, --diff, --no-back, --one-buf and --batch-check for the slave"):format(net.default_port))
	print("   --master[=port]", "find the slaves on the port. a track whose --cfg surface is on a screen of a slave plays on that slave, the rest play here. the slaves follow the clock of the master")
	return
end
if ops.v or ops.version then
	print(("SZT Stream Reader V%s"):format(version))
	print(("understands szt%s containers"):format(szt.version))
	return
end

local gpu = component.gpu
video.init_gpus(gpu)
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
local path = args[1] or type(ops.tape) == "string" and ops.tape or nil --'--tape=<addr>' names the drive too
local function open_file()
	return linear_stream.open(path, "rb", ops.tape)
end
local file
if not ops.slave then --a slave plays the file that the master copied onto its tape
	local reason
	file, reason = open_file()
	if not file then
		error("Failed to open file: " .. reason)
	end
end

local res_x, res_y = gpu.getResolution()
local function errHandler(err)
	local text = net.error_text(err)
	if text then return text end
	gpu.setBackground(0xff0000)
	gpu.setForeground(0xffffff)
	return debug.traceback(err)
end

--an error here is outside the xpcall below, so print it plainly instead of crashing the shell
local link
do
	local ok, err = pcall(function()
		if ops.slave then
			link = net.slave(net.port(ops.slave), tape_given)
		elseif ops.master and not (ops.p or ops.probe) then
			link = net.master(net.port(ops.master))
			--the slaves read the file from the same place: the same path, or a tape with the same label
			if ops.tape then
				local drive = component.proxy(path and component.get(path, "tape_drive") or default_tape_drive())
				local label = drive and drive.getLabel()
				link.source = {label = label ~= "" and label or net.default_label}
			else
				link.source = {path = shell.resolve(path)}
			end
			link.ops = {}
			for _, k in ipairs(net.shared_ops) do
				link.ops[k] = ops[k]
			end
		end
	end)
	if not ok then
		if file then
			file:close()
		end
		print(tostring(err))
		return
	end
end

--a slave plays one session per master, until it is interrupted
local function slave_main()
	while true do
		local ok, err = xpcall(link.wait_setup, errHandler, link)
		if ok then
			if not err then return end --interrupted
			local setup = err
			ok, err = xpcall(function()
				--the master controls how the slave plays
				for _, k in ipairs(net.shared_ops) do
					ops[k] = setup.ops and setup.ops[k]
				end
				--the master sends the file first, unless the slave has it already
				local src, is_tape = link:fetch(setup)
				local function open_source()
					return linear_stream.open(src, "rb", is_tape)
				end
				local file = assert(open_source())
				play(gpu, file, setup.surfaces, open_source, link)
				file:close()
			end, errHandler)
		end
		if ok then
			if not ops.no_back then
				gpu.setActiveBuffer(0)
			end
		else
			video.reset(gpu)
			link:report(err)
			print("session ended: " .. tostring(err))
			if tostring(err):find("interrupted") then
				link.quit = true
			end
		end
		link:end_session()
		if link.quit then return end
	end
end

local function run()
	if ops.p or ops.probe then
		probe_header(file)
		return
	end
	if ops.slave then
		slave_main()
		return
	end

	play(gpu, file, surfaces, open_file, link)
end

local result, reason = xpcall(run, errHandler)
pcm.deinit() --don't leave the tape playing after an error
if link then
	link:close() --stops the slaves after an error
end

if file then
	file:close()
end
if ops.p or ops.probe then return end

--do cleanup
if not result then
	video.reset(gpu)
elseif not ops.no_back then
	gpu.setActiveBuffer(0)
end
gpu.setResolution(res_x, res_y)

--handle error
if not result then
	print("ERR:")
	print(reason)
	return
end

print("Done!")
