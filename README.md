# howlto

一个命令行工具, 帮你快速找到想要使用的命令.

Fast 🚀, cheap 🤩, and low-spec 🗿 -- driven perfectly by `gpt-4o-mini`.

## Demo

<video src="https://private-user-images.githubusercontent.com/60778594/527598361-ffc3b745-0be1-4c98-8a6c-95c56311ae2e.mp4?jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJnaXRodWIuY29tIiwiYXVkIjoicmF3LmdpdGh1YnVzZXJjb250ZW50LmNvbSIsImtleSI6ImtleTUiLCJleHAiOjE3NjU5Nzk5MDksIm5iZiI6MTc2NTk3OTYwOSwicGF0aCI6Ii82MDc3ODU5NC81Mjc1OTgzNjEtZmZjM2I3NDUtMGJlMS00Yzk4LThhNmMtOTVjNTYzMTFhZTJlLm1wND9YLUFtei1BbGdvcml0aG09QVdTNC1ITUFDLVNIQTI1NiZYLUFtei1DcmVkZW50aWFsPUFLSUFWQ09EWUxTQTUzUFFLNFpBJTJGMjAyNTEyMTclMkZ1cy1lYXN0LTElMkZzMyUyRmF3czRfcmVxdWVzdCZYLUFtei1EYXRlPTIwMjUxMjE3VDEzNTMyOVomWC1BbXotRXhwaXJlcz0zMDAmWC1BbXotU2lnbmF0dXJlPWFjMWUyNTQ0OGQxNWU2ZjNjMWUwZjlmNTllMjg4YjNlMjg2MWMyMmVmNDBjZTlkMzMxMjYyZTg3ODZjMDVjNTYmWC1BbXotU2lnbmVkSGVhZGVycz1ob3N0In0.uyvolXMKAnkVfI6SvtE8ZZuehgJMly9rR0_R5bkGPUw" title="Demo" autoplay muted loop></video>

<!-- todo sqlite3 demo -->

## 安装

```shell
cargo install --git https://github.com/azazo1/howlto.git
```

或者查看 Releases 中预编译的可执行文件.

## 配置

默认配置目录: `~/.config/howlto/config.toml` (Windows 下 `~` 为 `%USERPROFILE%`).

先启动一次 `howlto` 创建配置文件 (可以忽略此次报错),
修改配置文件中的 `api_key` 和 `base_url` (目前只支持 openai 格式, 需要 `/v1` 后缀),
然后就能正常使用了.

> [!NOTE]
> 项目仍然处于非常初步的阶段, 可能会引入许多 breaking changes💥, 因此可能在某次更新后需要手动调整配置内容以继续使用.

## 使用

直接像普通的英语句子一样提问:

```shell
howlto upgrade all bun packages
```

> How to upgrade all bun packages?

你会获得一个选项框, 可以复制, 编辑, 执行模型提供的命令等.

如果你只想获取纯净的输出, 且无需交互:

```shell
howlto -pq list all files
```

直接连接管道并执行:

```shell
howlto -pq list all files | head -n 1 | xargs $SHELL -c
```

> [!WARNING]
> 这个命令会直接执行模型输出的结果, 务必谨慎小心.

如果你想手动附加某些帮助信息作为辅助:

```shell
cargo --help | howlto cargo build with no default feature
```

### Shell 集成

以 fish 为例子, 执行下面的命令:

```fish
command howlto --init | source
```

来进行 shell 集成, 你也可以把这行初始化命令添加到 `~/.config/fish/config.fish` 这类文件中.

> 其他 shell 的集成命令参考: [Shell 集成](docs/shell-integration.md).

shell 集成之后, 可以使用以下 features:

- `Enter` 选择命令后自动填写到输入缓冲区.
- 自动修复上一条命令

  ```shell
  git psh -to origin main
  howl
  # git push -u origin main
  ```

  > 就像在提问: How?

## 🏁 Todo

见: [Todo](docs/todo.md)

## Issues

只有 macOS Apple Silicon 被测试运行过, 其他平台上的使用如遇问题欢迎提出 issues.

虽然该工具可在 Windows 环境下运行, 但在 Unix 系统 (如 macOS 或 Linux) 中的表现更为出色.
为了获得最佳交互体验, 强烈建议在 Unix 系统配合 fish shell 使用.
Windows 可以在 [WSL](https://learn.microsoft.com/zh-cn/windows/wsl/install) 下使用获取最佳的使用体验.

## Referencing

- ⭐[TheR1D/shell_gpt](https://github.com/TheR1D/shell_gpt): howlto is inspired by this repo.
- 🦆[szyazi/yazi](https://github.com/sxyazi/yazi): for it's shell integration method.
- 🧠[ajeetdsouza/zoxide](https://github.com/ajeetdsouza/zoxide): for it's shell integration method.
- 🗯️[nvbn/thefuck](https://github.com/nvbn/thefuck): for it's shell integration method and fixing commands ideas.
- 📒[tldr-pages/tldr](https://github.com/tldr-pages/tldr): for it's TL;DR content.

## Enjoy :)
