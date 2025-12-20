local HOWLTO_EXE = "__howlto_path__"
local function read_file(path)
    local f = io.open(path, "r")
    if not f then
        return nil
    end
    local content = f:read("*a")
    f:close()
    if content then
        return content:gsub("[\n\r]*$", "")
    end
    return nil
end
local function invoke_howlto(args)
    local tmp_file = os.getenv("TEMP") .. "\\howlto_" .. os.time() .. ".tmp"
    -- 注意: 需要小心处理 args 中的引号，这里做简单处理
    local cmd = string.format('"%s" %s --htcmd-file="%s"', HOWLTO_EXE, args, tmp_file)
    local success = os.execute(cmd)
    if success then
        local new_cmd = read_file(tmp_file)
        if new_cmd and #new_cmd > 0 then
            rl_state.line_buffer = new_cmd
            rl_state.point = #new_cmd
        end
    end
    os.remove(tmp_file)
end
local function on_filter_input(key, map)
    if key == "Return" or key == "NUMPAD_ENTER" then
        local line = rl_state.line_buffer
        local args = nil
        local is_howl = false
        if line == "howl" or line:match("^howl%s+") then
            local history_size = clink.history_size()
            if history_size > 0 then
                local last_cmd = clink.history_get(history_size) -- 获取最后一条
                if last_cmd then
                    local extra_context = line:match("^howl%s+(.*)") or ""
                    args = string.format('"I mistyped, fixit: `%s`." %s', last_cmd:gsub('"', '\\"'), extra_context)
                    is_howl = true
                end
            end
        elseif line:match("^howlto%s+") then
            args = line:match("^howlto%s+(.*)")
            is_howl = true
        end
        if is_howl and args then
            invoke_howlto(args)
            return true
        end
    end
end
clink.onfilterinput(on_filter_input)
