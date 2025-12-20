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
    local tmp_file = os.getenv("TEMP") .. "\\howlto_" .. os.getenv("RANDOM") .. ".tmp"
    -- 注意: 需要小心处理 args 中的引号，这里做简单处理
    local cmd = string.format('""%s" %s --htcmd-file="%s""', HOWLTO_EXE, args, tmp_file)
    -- clink.print(cmd)
    local success = os.execute(cmd)
    if success then
        local new_cmd = read_file(tmp_file)
        if new_cmd then
            -- 必须是快捷键绑定函数才能获取 rl_buffer, 这里无法使用.
            -- rl_buffer:insert(new_cmd)
        end
    end
    os.unlink(tmp_file)
end
local function on_filter_input(input_buffer)
    local line = input_buffer
    local args = nil
    local is_howl = false
    if line == "howl" or line:match("^howl%s+") then
        local hc = rl.gethistorycount()
        local history = rl.gethistoryitems(hc, hc)
        for _, prev_command in pairs(history) do
            local extra_context = line:match("^howl%s+(.*)") or ""
            args = string.format('"I mistyped, fixit: `%s`." %s', prev_command.line:gsub('"', '\\"'), extra_context)
            is_howl = true
            break
        end
    elseif line == "howlto" then
        args = ""
        is_howl = true
    elseif line:match("^howlto%s+") then
        args = line:match("^howlto%s+(.*)")
        is_howl = true
    end
    if is_howl and args then
        invoke_howlto(args)
        return {"", false}
    end
end
clink.onfilterinput(on_filter_input)
