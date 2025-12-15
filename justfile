default: howlto

alias m := mocker
mocker:
    cargo run --bin mocker --features mocker -- --config ./debug_config/mocker.toml

alias ht := howlto
howlto *ARGS:
    cargo run --bin howlto -- --config ./debug_config {{ARGS}}
