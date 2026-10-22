-- End-of-frame reference capture for run_timeline.ps1 (MAME 0.289).
-- Loaded with -autoboot_delay 0, before the first emulated instruction.
-- Frame 1 ends at the first machine-frame notifier. MAME polls physical inputs
-- before this notifier, so field overrides are queued two frames ahead. Their
-- resulting cached port values are checked before the next CPU frame runs.
local mach = manager.machine
local out, finished

local function fail(message)
    if finished then return end
    finished = true
    if out then out:close(); out = nil end
    local errors = io.open("timeline.error.txt", "w")
    if errors then errors:write(tostring(message), "\n"); errors:close() end
    emu.print_error(tostring(message))
    mach:exit()
end

local function inconclusive(message)
    if finished then return end
    finished = true
    if out then out:close(); out = nil end
    local status = assert(io.open("timeline.inconclusive.txt", "w"))
    status:write(message, "\n"); status:close()
    mach:exit()
end

local function main()
    local options = assert(loadfile("timeline_options.lua"))()
    assert(math.type(options.frames) == "integer" and options.frames > 0,
        "frames must be a positive integer")
    assert(math.type(options.script_start) == "integer" and options.script_start > 0,
        "script_start must be a positive integer")
    assert(emu.time() == 0, "capture must be installed at power-on (use -autoboot_delay 0)")

    local steps = {}
    for line in io.lines("inputscript.txt") do
        line = line:gsub("#.*$", ""):match("^%s*(.-)%s*$")
        if line ~= "" then
            local b, n = line:match("^(%x+)%s+(%d+)$")
            local buttons, frames = tonumber(b, 16), tonumber(n)
            assert(buttons and buttons <= 255 and frames and frames > 0
                and math.type(frames) == "integer", "invalid input script line: " .. line)
            steps[#steps + 1] = { buttons = buttons, frames = frames }
        end
    end

    -- These are physical active-low switch values. The trace uses the native
    -- hardware constants' inverted polarity (66/DB); the game receives 99/24.
    local function pin_dips(tag, value)
        local port = assert(mach.ioport.ports[tag], "missing port " .. tag)
        for _, field in pairs(port.fields) do field.user_value = value & field.mask end
        assert((port:read() & 255) == value, "DIP setup mismatch on " .. tag)
    end
    pin_dips(":DSWA", 0x99)
    pin_dips(":DSWB", 0x24)

    local seed = assert(io.open("nvram/digdug/earom", "rb"))
    local earom = seed:read("a"); seed:close()
    assert(earom == string.rep("\0", 64), "EAROM seed must be exactly 64 zero bytes")

    local hash = 2166136261
    for _, cpu in ipairs({ { ":maincpu", 0x4000 }, { ":sub", 0x2000 }, { ":sub2", 0x1000 } }) do
        local space = assert(mach.devices[cpu[1]], "missing CPU " .. cpu[1]).spaces["program"]
        for address = 0, cpu[2] - 1 do
            hash = ((hash ~ space:read_u8(address)) * 16777619) & 0xFFFFFFFF
        end
    end

    local function field(tag, name)
        return assert(mach.ioport.ports[tag].fields[name], "missing input " .. name)
    end
    -- Override every digital input so keyboard/joystick state cannot contaminate
    -- a reference run. Scripted inputs are all for the upright player-1 controls.
    for _, tag in ipairs({ ":IN0", ":IN1" }) do
        for _, input in pairs(mach.ioport.ports[tag].fields) do input:set_value(0) end
    end
    local inputs = {
        { 0x01, field(":IN0", "P1 Up") },
        { 0x02, field(":IN0", "P1 Right") },
        { 0x04, field(":IN0", "P1 Down") },
        { 0x08, field(":IN0", "P1 Left") },
        { 0x10, field(":IN1", "P1 Button 1") },
        { 0x20, field(":IN1", "1 Player Start") },
        { 0x40, field(":IN1", "Coin 1") },
        { 0x80, field(":IN1", "2 Players Start") },
    }
    local function buttons_for(frame)
        if frame < options.script_start then return 0 end
        local elapsed = frame - options.script_start
        for _, step in ipairs(steps) do
            if elapsed < step.frames then return step.buttons end
            elapsed = elapsed - step.frames
        end
        return 0
    end
    local function apply(buttons)
        for _, input in ipairs(inputs) do
            input[2]:set_value((buttons & input[1]) ~= 0 and 1 or 0)
        end
    end
    local function check_physical_inputs(frame)
        local requested = buttons_for(frame)
        local expected0 = (~requested) & 0x0F | 0xF0
        local active1 = ((requested & 0x10) ~= 0 and 0x01 or 0)
            | ((requested & 0x20) ~= 0 and 0x04 or 0)
            | ((requested & 0x40) ~= 0 and 0x10 or 0)
            | ((requested & 0x80) ~= 0 and 0x08 or 0)
        local expected1 = (~active1) & 0xFF
        local actual0 = mach.ioport.ports[":IN0"]:read() & 0xFF
        local actual1 = mach.ioport.ports[":IN1"]:read() & 0xFF
        if actual0 ~= expected0 or actual1 ~= expected1 then
            inconclusive(string.format("Physical input mismatch before frame %d (buttons %02X): IN0/IN1 %02X/%02X, expected %02X/%02X. Frame-one nonzero input and unsupported frontend direction combinations cannot be compared.",
                frame, requested, actual0, actual1, expected0, expected1))
            return false
        end
        return true
    end

    -- The initial input poll has already happened when autoboot runs at time 0.
    -- Never label a requested frame-one press as present when its physical port
    -- is still neutral. Such a script is explicitly inconclusive.
    if not check_physical_inputs(1) then return end

    out = assert(io.open("reference.timeline", "w"))
    out:write("# digdug-timeline-v2\n")
    out:write(string.format("# dip_a=66 dip_b=DB earom=%s script_start=%d frames=%d rom_hash=%08X\n",
        string.rep("00", 64), options.script_start, options.frames, hash))
    local mem = mach.devices[":maincpu"].spaces["program"]
    local hex = {}
    for byte = 0, 255 do hex[byte] = string.format("%02X", byte) end
    local frame, buttons = 0, buttons_for(1)
    -- Current physical cache is for frame 1. Queue frame 2 for the first poll.
    -- MAME 0.289 ioport_field::set_value only changes the pending override;
    -- ioport_port::frame_update rebuilds the cache before Lua frame notifiers.
    -- https://github.com/mamedev/mame/blob/mame0289/src/emu/ioport.cpp
    apply(buttons_for(2))

    timeline_frame_subscription = emu.add_machine_frame_notifier(function()
        if finished then return end
        local ok, message = xpcall(function()
            frame = frame + 1
            -- The core's poll precedes this callback. Verify that it installed
            -- the next frame's inputs before recording the frame just completed.
            if frame < options.frames and not check_physical_inputs(frame + 1) then return end
            local ram = {}
            for address = 0x8000, 0x9FFF do ram[#ram + 1] = hex[mem:read_u8(address)] end
            out:write(string.format("%d %02X %s\n", frame, buttons, table.concat(ram)))
            if frame == options.frames then
                out:close(); out = nil
                local complete = assert(io.open("timeline.complete", "w"))
                complete:write(tostring(frame), "\n"); complete:close()
                finished = true
                apply(0)
                mach:exit()
            else
                buttons = buttons_for(frame + 1)
                -- set_value changes the pending override, not the live digital
                -- port cache. That cache will rebuild at the NEXT frame end.
                apply(buttons_for(frame + 2))
            end
        end, debug.traceback)
        if not ok then fail(message) end
    end)
end

local ok, message = xpcall(main, debug.traceback)
if not ok then fail(message) end
