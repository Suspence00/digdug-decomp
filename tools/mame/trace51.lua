-- Applies mame_ref/inputscript.txt to MAME's Dig Dug and logs every main-CPU access to the
-- Namco 06XX/51XX window ($7000..$71FF), tagged by script step. Output: out/trace51_mame.txt
local START_FRAME = 700   -- let MAME finish its power-on self-test before the script begins
local mach = manager.machine
local mem = mach.devices[":maincpu"].spaces["program"]
local out = io.open("out/trace51_mame.txt", "w")

-- Load script
local steps = {}
for line in io.lines("inputscript.txt") do
  line = line:match("^%s*(.-)%s*$")
  if line ~= "" and line:sub(1, 1) ~= "#" then
    local b, n = line:match("^(%x+)%s+(%d+)")
    steps[#steps + 1] = { buttons = tonumber(b, 16), frames = tonumber(n) }
  end
end

local function field(tag, name) return mach.ioport.ports[tag].fields[name] end
local map = {
  { 0x01, field(":IN0", "P1 Up") },
  { 0x02, field(":IN0", "P1 Right") },
  { 0x04, field(":IN0", "P1 Down") },
  { 0x08, field(":IN0", "P1 Left") },
  { 0x10, field(":IN1", "P1 Button 1") },
  { 0x20, field(":IN1", "1 Player Start") },
  { 0x40, field(":IN1", "Coin 1") },
}
local function apply(buttons)
  for _, m in ipairs(map) do m[2]:set_value((buttons & m[1]) ~= 0 and 1 or 0) end
end

local frame, step, step_end = 0, -1, 0
local function cur_step() return step < 0 and "pre" or tostring(step) end

read_tap = mem:install_read_tap(0x7000, 0x71FF, "t51r", function(offset, data, mask)
  if step >= 0 then out:write(string.format("S%s R %04X %02X\n", cur_step(), offset, data & 0xFF)) end
  return data
end)
write_tap = mem:install_write_tap(0x7000, 0x71FF, "t51w", function(offset, data, mask)
  if step >= 0 then out:write(string.format("S%s W %04X %02X\n", cur_step(), offset, data & 0xFF)) end
  return data
end)

frame_sub = emu.add_machine_frame_notifier(function()
  frame = frame + 1
  if frame == START_FRAME then
    step = 0; step_end = frame + steps[1].frames; apply(steps[1].buttons)
  elseif frame > START_FRAME and step >= 0 and frame >= step_end then
    step = step + 1
    if step >= #steps then
      apply(0); out:close(); mach:exit(); return
    end
    step_end = frame + steps[step + 1].frames
    apply(steps[step + 1].buttons)
  end
end)
