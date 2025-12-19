default: howlto

profile := "dev"

alias m := mocker
mocker:
    cargo run --bin mocker --features mocker --profile {{profile}} -- --config ./debug_config/mocker.toml

alias ht := howlto
howlto *ARGS:
    cargo run --bin howlto --profile {{profile}} -- --config ./debug_config {{ARGS}}

nushell build='0':
    if [ '{{build}}' = '1' ]; then \
        docker build -f Dockerfile_nushell -t howlto-nushell .; \
    fi
    docker run -it --rm -v ./debug_config:/root/.config/howlto/ howlto-nushell nu

clean:
    docker image rm howlto-nushell
    cargo clean