# 使用场景

## 直接传参

```shell
howlto check disk usage of .
howlto docker build Dockerfile_nushell to name howlto-nushell
howlto docker run howlto-nushell with oneshot
howlto zip . to a.zip and dont delete any file, exclude ./target/
howlto list zip a.zip
howlto ls only files with time asc
```

## 修改

```shell
howlto list file real sizes in howlto-windows-x86_64.zip
# 输出: unzip -l howlto-windows-x86_64.zip
# (m)修改 Prompt: convert sizes to human readable.
# 输出: unzip -l howlto-windows-x86_64.zip | awk 'NR>3 { printf "% -40s % 8.1f %s\n", $4, $1/1024/1024, "MB" }'
```

## 通过管道提供辅助信息

```shell
bun --help | howlto update bun itself
```

## Shell 集成

> [!NOTE]
> 请先进行 Shell 集成, 才能使用下面的功能, 见 [Shell 集成](shell-integration.md).

### 自动修改上一个命令

```shell
crgo install .
howl
# 模型输出: cargo install . (或者 cargo install --path .)
cargo install .
howl
# 模型查阅 cargo 帮助, 给出 cargo install --path .
cargo install --path .
```
