function __howlto_invoke --description 'howlto with shell integration'
    set program __howlto_path__
    set tmp (mktemp -t "howlto-tmp.XXXXXX")
    "$program" $argv --htcmd-file="$tmp"
    if [ $status != 0 ]
        rm -f -- "$tmp"
        return $status
    end
    if read -z htcmd <"$tmp"; and [ -n "$htcmd" ]
        commandline -r "$htcmd"
    end
    rm -f -- "$tmp"
end
function howl --description 'fix last command'
    set argv "I mistyped, fixit: \`$history[1]\`." $argv
    __howlto_invoke $argv
end
alias howlto __howlto_invoke
