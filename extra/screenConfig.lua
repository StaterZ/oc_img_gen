local event = require("event")
local term = require("term")
local io = require("io")
local serialization = require("serialization")
local unicode = require("unicode")
local component = require("component")
local pc = require("computer")

local gpu = component.gpu

local function ask(question)
	term.write(question .. ":")
	local response = term.read()
	response = string.sub(response, 1, #response - 1) --remove the stupid newline at the end
	return response
end

local function set_screen_message(addr, msg)
	local width, height = unicode.wlen(msg) + 2, 3
	if addr ~= gpu.getScreen() then
		local bg, fg = gpu.getBackground(), gpu.getForeground()
		gpu.bind(addr, false)
		gpu.setBackground(bg)
		gpu.setForeground(fg)
	end
	gpu.setResolution(width, height)
	gpu.fill(1, 1, width, height, " ")
	gpu.set(2, 2, msg)
end

local function grab_resolutions(msg)
	local result = {}
	for addr, _kind in component.list("screen") do
		gpu.bind(addr, false)
		local width, height = gpu.getResolution()
		result[addr] = {
			x = width,
			y = height,
		}

		if msg then
			gpu.setBackground(0x0000ff)
			gpu.setForeground(0xffffff)
			set_screen_message(addr, msg)
		end
	end
	return result
end

local function restore_resolutions(resolutions)
	local result = {}
	for addr, resolution in pairs(resolutions) do
		gpu.bind(addr, false)
		gpu.setResolution(resolution.x, resolution.y)
		gpu.setBackground(0x000000)
		gpu.fill(1, 1, resolution.x, resolution.y, " ")
	end
	return result
end

local function get_screen(msg, blacklist)
	local sizes = {}
	
	pc.beep(1750, 0.05)
	local addr
	while true do
		local e = {event.pull()}
		
		if e[1] == "touch" then
			addr = e[2]
			if not blacklist[addr] then break end
		end
	end
	
	gpu.setBackground(0x00ff00)
	gpu.setForeground(0xffffff)
	set_screen_message(addr, msg)
	
	return addr
end

local stream_size = {
	x = tonumber(ask("Stream Width")),
	y = tonumber(ask("Stream Height"))
}
local matrix_size = {
	x = tonumber(ask("Matrix Width")),
	y = tonumber(ask("Matrix Height"))
}

print("starting screen selector...")
local main_screen = gpu.getScreen()
local resolutions = grab_resolutions("Touch screen to select")
local blacklist = {}
local surfaces = {}
for y = 1, matrix_size.y do
	for x = 1, matrix_size.x do
		local name = ("%i,%i"):format(x - 1, y - 1)
		local screen_addr = get_screen(
			("Surface '%s'"):format(name),
			blacklist
		)
		blacklist[screen_addr] = true
		
		surfaces[name] = {
			screen_addr = screen_addr,
			is_fullscreen = true,
		}
	end
end
restore_resolutions(resolutions)

gpu.bind(main_screen, false)
gpu.setBackground(0x000000)
gpu.setForeground(0xffffff)
term.setCursor(1, 1)
print("Configuration complete!")

local path = ask("save path")
local file, reason = io.open(path, "w")
if not file then
	error("failed opening file for writing: " .. reason)
end

file:write(serialization.serialize(surfaces))

file:close()
