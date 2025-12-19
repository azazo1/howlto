# referencing yazi
function __howlto_invoke --description 'howlto with shell integration'
    set program __howlto_path__
    set tmp (mktemp -t "howlto-tmp.XXXXXX")
    $program $argv --htcmd-file="$tmp"
    if [ $status != 0 ]
        return $status
    end
    if read -z htcmd <"$tmp"
        commandline -r "$htcmd"
    end
    rm -f -- "$tmp"
end

alias howlto __howlto_invoke
