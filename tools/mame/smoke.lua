-- Smoke test: list input fields, run N frames, dump shared RAM ($8000-$9FFF) at a few frames.
local out = io.open("out/smoke_out.txt", "w")
local mach = manager.machine
local cpu = mach.devices[":maincpu"]
local mem = cpu.spaces["program"]

for tag, port in pairs(mach.ioport.ports) do
  for name, field in pairs(port.fields) do
    out:write(string.format("FIELD %s : %s (mask %02X)\n", tag, name, field.mask))
  end
end

local dump_frames = { [60] = true, [300] = true, [600] = true }
local last = 600
local frame = 0
frame_sub = emu.add_machine_frame_notifier(function()
  frame = frame + 1
  if dump_frames[frame] then
    local t = {}
    for a = 0x8000, 0x9FFF do t[#t + 1] = string.format("%02X", mem:read_u8(a)) end
    out:write(string.format("RAM frame=%d %s\n", frame, table.concat(t)))
  end
  if frame >= last then
    out:close()
    mach:exit()
  end
end)
