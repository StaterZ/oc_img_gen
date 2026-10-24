local io = require("io")
local fs = require("filesystem")
local shell = require("shell")
local component = require("component")
local computer = require("computer")

local args, ops = shell.parse(...)

if ops.h or ops.help or not args[1] then
	print("write_szt_tape <file> [drive address...]")
	print("writes a szt file to the start of tapes so 'play_szt.lua --tape' can play it")
	print("default drive is the primary. give several to fill tapes for 'play_szt_par.lua --tape'")
	print("   --no-label", "don't set the tape label to the file name")
	return
end

local path = shell.resolve(args[1])
local file_size = fs.size(path)

local drives = {}
if #args > 1 then
	for i = 2, #args do
		local addr = assert(component.get(args[i], "tape_drive"))
		table.insert(drives, component.proxy(addr))
	end
elseif component.isAvailable("tape_drive") then
	table.insert(drives, component.tape_drive)
else
	error("no tape drive found")
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
