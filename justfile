default: howlto

alias m := mocker
mocker:
    cargo run --bin mocker --features mocker

alias ht := howlto
howlto *ARGS:
    cargo run --bin howlto -- {{ARGS}}