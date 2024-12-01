local os = require("os")
local shell = require("shell")
local event = require("event")
local unicode = require("unicode")
local comp = require("component")

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

local function draw(gpu, file, pos_x, pos_y)
	do
		local magic = file:read(4)
		assertEq(magic, "sztb", "bad magic")
		local version = read_u16(file)
		assertEq(version, 2, "bad version")
	end

	local back
	do
		local w = read_u8(file)
		local h = read_u8(file)
		gpu.setResolution(w, h)

		if not ops.noback then
			back = gpu.allocateBuffer(w, h)
			gpu.setActiveBuffer(back)
		end
	end

	local frame_rate = read_u16(file)
	local num_frames = read_u32(file)

	print("reading seek table...", num_frames)
	local seek_table = {}
	seek_table[0] = 0 --sneaky pls no crash fix
	for i = 1, num_frames do
		seek_table[i] = seek_table[i - 1] + read_u32(file)
	end
	local frames_begin_pos = file:seek()
	print("done at", frames_begin_pos)

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

		local commands_len = seek_table[frame_index + 1] - seek_table[frame_index]
		local command_kind = read_u8(file)
		local i = 1 -- 1 since we include the command_kind u8
		while i < commands_len do
			local len = read_u8(file)
			i = i + 1

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

			local x = read_u8(file)
			local y = read_u8(file)
			
			len = len + 1
			local value
			-- if command_kind == 1 then
				value = ""
				for j = 1, len do
					value = value .. unicode.char(0x2800 + read_u8(file))
				end
			-- else
			-- 	value = file:read(len)
			-- else
			gpu.set(x + pos_x, y + pos_y, value)
			i = i + 2 + len
		end

		if not ops.fast then
			repeat
				local current_time = (os.clock() - begin_time)
				local next_frame_index = math.ceil(current_time * frame_rate)
			until next_frame_index > frame_index
		end

		if ops.fps then
			gpu.setBackground(0xff0000)
			gpu.setForeground(0xffffff)
			local now = os.clock()
			local elapsed = now - frame_begin_time
			gpu.set(1, 1, ("%04i %04.1flag %03.ffps %05.fms %05ib"):format(
				frame_index,
				frame_index / frame_rate - (now - begin_time),
				1 / elapsed,
				elapsed * 1000,
				commands_len
			))
		end

		if back ~= nil then
			gpu.bitblt()

			if ops.diff then
				gpu.setBackground(0x000000)
				gpu.setForeground(0xff0000)
				local w, h = gpu.getResolution()
				gpu.fill(1, 1, w, h, "*")
			end
		end

		while true do
			local e = event.pull(0)
			if e == nil then
				break
			elseif e == "interupted" then
				goto done
			end
		end

		::continue::
		frame_index = frame_index + 1
	end
	::done::

	if back ~= nil then
		gpu.freeBuffer(back)
		gpu.setActiveBuffer(0)
	end
end


local path = args[1]
local file, reason = io.open(path, "rb")
if not file then
	error("Failed to open file: " .. reason)
end

local ok, reason = pcall(draw, comp.gpu, file, 1, 1)
file:close()

comp.gpu.setBackground(0xff0000)
comp.gpu.setForeground(0xffffff)
require("term").setCursor(1, 1)

if not ok then
	print("ERR:")
	print(reason)
	return
end

print("Done!")
