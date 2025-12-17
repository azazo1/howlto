default: howlto

profile := "dev"

alias m := mocker
mocker:
    cargo run --bin mocker --features mocker --profile {{profile}} -- --config ./debug_config/mocker.toml

alias ht := howlto
howlto *ARGS:
    cargo run --bin howlto --profile {{profile}} -- --config ./debug_config {{ARGS}}
