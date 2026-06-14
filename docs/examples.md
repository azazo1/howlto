# 使用场景

## 直接传参

```shell
howlto check disk usage of .
howlto docker build Dockerfile_nushell to name howlto-nushell
howlto docker run howlto-nushell with oneshot
howlto zip . to a.zip and dont delete any file, exclude ./target/
howlto list zip a.zip
howlto ls only files with time asc
howlto convert a.mov to a.webp with fps 10, quality 50, width 500 and height auto
howlto git remove all reflog
howlto qpdf list all objects in a.pdf
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

## 直接回答简单问题

这类问题通常不需要把命令交给用户选择, 而是直接给出文本结果. 如果 agent 需要查看当前环境, 会先使用只读工具获取结果, 再把关键步骤和结论一起输出.

```shell
howlto explain why git says detached HEAD
# 可能输出: detached HEAD 表示当前检出的是某个提交而不是分支. 如果只是查看代码可以忽略, 如果要继续开发可以新建分支.

howlto is this repo dirty
# 可能输出: 工作区是干净的. 关键步骤: git status --short

howlto what is the current package version
# 可能输出: 当前包版本是 0.2.1. 关键步骤: 读取 Cargo.toml 中的 package.version

howlto why does cargo say package not found
# 可能输出: 通常是包名写错, registry 没有同步, 或者当前 source 配置不对. 先确认包名和 registry source.
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
