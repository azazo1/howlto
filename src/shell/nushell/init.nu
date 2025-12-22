def __howlto_invoke [...args] {
    let program = "__howlto_path__"
    let tmp = (mktemp -t "howlto-tmp.XXXXXX")
    try {
        ^$program --htcmd-file $tmp ...$args
    } catch {
        rm -f $tmp
        error make {msg: "Failed", label: {text: "Failed to execute howlto", span: (metadata $fail).span}}
    }
    let exit_status = $env.LAST_EXIT_CODE
    if $exit_status != 0 {
        rm -f $tmp
        error make {msg: "Howlto Error", label: {text: "Howlto returned: $exit_status", span: (metadata $fail).span}}
    }
    if ($tmp | path exists) {
        let htcmd = (open $tmp | str trim)
        if ($htcmd | str length) > 0 {
            commandline edit --replace $htcmd
        }
    }
    rm -f $tmp
}
def howl [...args] {
    let last_cmd = (history | last 1 | get command | get 0)
    let fix_msg = $"I mistyped, fixit: `($last_cmd)`."
    __howlto_invoke $fix_msg ...$args
}
alias howlto = __howlto_invoke