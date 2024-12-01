local os = require("os")
local shell = require("shell")
local event = require("event")
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
		assertEq(version, 1, "bad version")
	end

	local back
	do
		local w = read_u8(file)
		local h = read_u8(file)
		gpu.setResolution(w, h)

		if ops.back then
			back = gpu.allocateBuffer(w, h)
			gpu.setActiveBuffer(back)
		end
	end

	local frame_rate = read_u16(file)
	local num_frames = read_u64(file)
	local next_frame = 0
	local begin_time = os.clock()
	for frame_index = 0, num_frames - 1 do
		local frame_begin_time = os.clock()
		local commands_len = read_u64(file)

		if ops.seek and frame_index < next_frame then
			file:seek("cur", commands_len)
			goto continue
		end

		local i = 0
		while i < commands_len do
			local cmd = read_u8(file)
			i = i + 1

			if cmd == 0x80 then
				gpu.setBackground(inflate(read_u8(file)))
				i = i + 1
			elseif cmd == 0x81 then
				gpu.setForeground(inflate(read_u8(file)))
				i = i + 1
			--elseif cmd == 0x82 then --we don't support resolution to increase speed
			--	local x = read_u8(file)
			--	local y = read_u8(file)
			--	gpu.setResolution(x, y)
			--	i = i + 2
			else
				local x = read_u8(file)
				local y = read_u8(file)
				local value = file:read(cmd)

				gpu.set(x + pos_x, y + pos_y, value)
				i = i + 2 + cmd
			end
		end

		local current_time = (os.clock() - begin_time)
		next_frame = math.ceil(current_time * frame_rate)
		
		if ops.fps then
			gpu.setBackground(0xff0000)
			gpu.setForeground(0xffffff)
			gpu.set(1, 1, ("%04i %05.f ms %05i b"):format(
				frame_index,
				(os.clock() - frame_begin_time) * 1000,
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

draw(comp.gpu, file, 1, 1)
file:close()

comp.gpu.setBackground(0xff0000)
comp.gpu.setForeground(0xffffff)
print("Done!")
