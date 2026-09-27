[private]
default:
    @just --list

profile := "dev"

alias ht := howlto
howlto *ARGS:
    cargo run --bin howlto --profile {{profile}} -- --config ./debug_config {{ARGS}}

nushell build='0':
    if [ '{{build}}' = '1' ]; then \
        docker build -f Dockerfile_nushell -t howlto-nushell .; \
    fi
    docker run -it --rm -v ./debug_config:/root/.config/howlto/ howlto-nushell nu

port:
    mkdir -p archives
    zip -r archives/howlto.zip . -x "./target/*" -x "./.git/*" -x "./archives/*"
    simple-http-server -- archives

# 根据当前平台构建并打包发布归档.
[linux]
dist:
    PROJECT_BUILD_VERSION="v$(bash scripts/build-version.sh)" bash scripts/dist.sh

# 根据当前平台构建并打包发布归档.
[macos]
dist:
    PROJECT_BUILD_VERSION="v$(bash scripts/build-version.sh)" bash scripts/dist.sh

clean:
    docker image rm howlto-nushell
    cargo clean