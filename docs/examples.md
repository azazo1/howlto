# 使用场景

## 直接传参

```shell
howlto check disk usage of .
howlto docker build Dockerfile_nushell to name howlto-nushell
howlto docker run howlto-nushell with oneshot
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
