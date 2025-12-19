function __howlto_invoke() {
    local program="__howlto_path__"
    local tmp=$(mktemp -t "howlto-tmp.XXXXXX")
    $program "$@" --htcmd-file="$tmp"
    local exit_status=$?
    if [ $exit_status -ne 0 ]; then
        rm -f -- "$tmp"
        return $exit_status
    fi
    if [ -s "$tmp" ]; then
        local htcmd=$(cat "$tmp")
        if [ -n "$htcmd" ]; then
            # 需要配合 bind -x '"\e-h": howlto' 使用才能生效.
            READLINE_LINE="$htcmd"
            READLINE_POINT=${#htcmd}
        fi
    fi
    rm -f -- "$tmp"
}
function howl() {
    local last_cmd=$(history 2 | head -n 1 | sed 's/^[ ]*[0-9]*[ ]*//')
    __howlto_invoke "I mistyped, fixit: \`$last_cmd\`." "$@"
}
alias howlto=__howlto_invoke
