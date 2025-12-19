function __howlto_invoke() {
    local program="__howlto_path__"
    local tmp=$(mktemp -t "howlto-tmp.XXXXXX")
    "$program" "$@" --htcmd-file="$tmp"
    local exit_status=$?
    if [ $exit_status -ne 0 ]; then
        rm -f -- "$tmp"
        return $exit_status
    fi
    if [ -s "$tmp" ]; then
        local htcmd=$(<"$tmp")
        if [ -n "$htcmd" ]; then
            print -rz -- "$htcmd"
        fi
    fi
    rm -f -- "$tmp"
}
function howl() {
    local last_cmd="$(fc -nl -1)"
    __howlto_invoke "I mistyped, fixit: \`$last_cmd\`." "$@"
}
alias howlto=__howlto_invoke