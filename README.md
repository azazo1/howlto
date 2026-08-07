# howlto

一个命令行工具, 帮你快速找到想要使用的命令.

Fast 🚀, cheap 🤩, and low-spec 🗿 -- driven perfectly by `gpt-4o-mini` or `gpt-5.4-mini`.

## Demo

<video src="https://private-user-images.githubusercontent.com/60778594/527598361-ffc3b745-0be1-4c98-8a6c-95c56311ae2e.mp4?jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJnaXRodWIuY29tIiwiYXVkIjoicmF3LmdpdGh1YnVzZXJjb250ZW50LmNvbSIsImtleSI6ImtleTUiLCJleHAiOjE3NjU5Nzk5MDksIm5iZiI6MTc2NTk3OTYwOSwicGF0aCI6Ii82MDc3ODU5NC81Mjc1OTgzNjEtZmZjM2I3NDUtMGJlMS00Yzk4LThhNmMtOTVjNTYzMTFhZTJlLm1wND9YLUFtei1BbGdvcml0aG09QVdTNC1ITUFDLVNIQTI1NiZYLUFtei1DcmVkZW50aWFsPUFLSUFWQ09EWUxTQTUzUFFLNFpBJTJGMjAyNTEyMTclMkZ1cy1lYXN0LTElMkZzMyUyRmF3czRfcmVxdWVzdCZYLUFtei1EYXRlPTIwMjUxMjE3VDEzNTMyOVomWC1BbXotRXhwaXJlcz0zMDAmWC1BbXotU2lnbmF0dXJlPWFjMWUyNTQ0OGQxNWU2ZjNjMWUwZjlmNTllMjg4YjNlMjg2MWMyMmVmNDBjZTlkMzMxMjYyZTg3ODZjMDVjNTYmWC1BbXotU2lnbmVkSGVhZGVycz1ob3N0In0.uyvolXMKAnkVfI6SvtE8ZZuehgJMly9rR0_R5bkGPUw" title="Demo" autoplay muted loop></video>

<!-- todo sqlite3 demo -->

## 安装

```shell
cargo install --git https://github.com/azazo1/howlto.git
```

或者查看 Releases 中预编译的可执行文件.

## 配置

默认不会创建 `config.toml` 或 `profiles.toml`. 文件不存在时使用内置配置和默认 profile, 文件已存在时仍会正常加载.

OpenAI 的 `base_url` 默认是 `https://api.openai.com/v1`, 模型默认是 `gpt-4o-mini`. 最简单的无配置文件用法只需要提供 API key:

```shell
env OPENAI_API_KEY=sk-... howlto explain detached HEAD
```

使用其他 OpenAI-compatible 服务时, 可以通过环境变量覆盖连接信息:

```shell
env HOWLTO_API_KEY=... HOWLTO_BASE_URL=http://localhost:8000/v1 HOWLTO_MODEL=my-model howlto list files
```

`HOWLTO_API_KEY`, `HOWLTO_BASE_URL`, `HOWLTO_MODEL`, `HOWLTO_LANGUAGE` 的优先级高于配置文件. 使用内置 OpenAI 默认配置时, API key, base URL 和 model 也分别兼容 `OPENAI_API_KEY`, `OPENAI_BASE_URL`, `OPENAI_MODEL`. 通用 `OPENAI_*` 变量不会覆盖已有的自定义 provider 配置.

需要持久化配置或自定义默认 prompt 时, 显式创建缺失文件:

```shell
howlto --init-config
```

该命令在默认配置目录 `~/.config/howlto/` 中创建缺失的 `config.toml` 和 `profiles.toml`, 已有文件不会被覆盖. 使用 `--config <dir>` 可以指定其他目录.

> [!NOTE]
> 项目仍然处于非常初步的阶段, 可能会引入许多 breaking changes💥, 因此可能在某次更新后需要手动调整配置内容以继续使用.

## 使用

直接像普通的英语句子一样提问:

```shell
howlto upgrade all bun packages
```

> How to upgrade all bun packages?

## 交互模式

直接运行 `howlto` 会进入交互模式. 如果当前工作目录已有会话, 会先显示会话菜单, 可以选择从头开始一个新会话, 或者继续某个历史会话.

```shell
howlto
```

普通单次执行, 包括 `--plain` 和管道附加输入, 也会自动把完整对话历史和最后一次输出保存到当前配置目录下的 `sessions/` 目录. 再次进入交互模式时即可继续.

会话按工作目录分组, 默认全局占用上限为 64 MiB. 可以在 `config.toml` 中通过 `[session]` 调整:

```toml
[session]
max_bytes = 67108864 # 整个 sessions 目录的最大字节数, 0 表示禁用
max_per_dir = 0 # 每个工作目录最多保留的会话数, 0 表示不限
ttl_days = 0 # 会话过期天数, 0 表示不过期
```

可以在 `config.toml` 中通过 `[agent]` 限制流式进度中的滚动字符速度:

```toml
[agent]
scroll_char_speed_limit = 0 # 滚动字符速度限制(字符/秒), 0 表示不限制
```

你会获得一个选项框, 可以复制, 编辑, 执行模型提供的命令等.

首个独立词可以作为输出偏好. `command`, `cmd`, `c` 偏向命令候选, `text`, `txt`, `t` 偏向纯文字回答:

```shell
howlto c list files by size
howlto t explain detached HEAD
```

如果你只想获取纯净的输出, 且无需交互:

```shell
howlto -pq list all files
```

存在候选命令时, `--plain` 会跳过选择界面, 并且只向 stdout 输出原始命令. 纯文本回答仍会移除 Markdown 标记后输出.

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

更多使用场景见: [示例](docs/examples.md).

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

### 平台支持情况

| 平台 | 状态 | 说明 |
| --- | --- | --- |
| **macOS** (Apple Silicon / Intel) | ✅ 主要测试平台 | 沙箱后端 Seatbelt (`sandbox-exec`). |
| **Linux** | ✅ 已支持 | 沙箱后端 Bubblewrap (`bwrap`), 需系统已安装 `bwrap`. |
| **Windows (原生)** | ⚠️ **当前无法编译** | 只读沙箱后端 (macOS Seatbelt / Linux Bubblewrap) 仅在 Unix 可用, 在 Windows 下会触发 [`compile_error!`](src/agent/sandbox.rs) 直接编译失败, 因此原生 Windows 暂时**无法构建/运行**. shell 集成也仅实现了 Unix 系 shell (fish/bash/zsh/nushell), 待沙箱后端与 Windows shell 集成落地后才能整体可用 (见 [Todo](docs/todo.md)). |
| **Windows (WSL)** | ✅ 推荐 | 在 [WSL](https://learn.microsoft.com/zh-cn/windows/wsl/install) 内按 Linux 方式使用即可获得最佳体验, 配合 fish shell 效果最好. |

> 项目对 Unix 系 (macOS / Linux) 的支持最为完善, 强烈建议在 Unix 系配合 fish shell 使用.

## Referencing

- ⭐[TheR1D/shell_gpt](https://github.com/TheR1D/shell_gpt): howlto is inspired by this repo.
- 🦆[szyazi/yazi](https://github.com/sxyazi/yazi): for it's shell integration method.
- 🧠[ajeetdsouza/zoxide](https://github.com/ajeetdsouza/zoxide): for it's shell integration method.
- 🗯️[nvbn/thefuck](https://github.com/nvbn/thefuck): for it's shell integration method and fixing commands ideas.
- 📒[tldr-pages/tldr](https://github.com/tldr-pages/tldr): for it's TL;DR content.

## Enjoy :)
