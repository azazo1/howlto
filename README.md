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

## 🏁 Todos

- [x] 简单调用, e.g: `howlto create a rust project?`.
- [x] 交互式选择, 修改, 直接执行或者复制输出的命令.
- [x] 自动读取命令的 `--help` / `man` / `tldr`.
- [x] 类似 `thefuck` 一样自动修复上一个在 shell 中执行的命令.
- [x] 添加 `thefuck` 帮助 tool.
- [x] 根据帮助文档识别子命令的帮助文档, 并根据子命令生成可用的命令.
- [ ] 多段对话功能, 在一次执行能多段对话, 并且在同一个 shell 中连续执行多次能够接上上面的对话内容 (detect_shell 中保存 shell 的 pid 作为键).
- [ ] shell 集成, 自动补全, 自动获取上一个命令的输出 (使用一个参数或者 shell 命令类似 `last_output | howlto ...` 控制这点).
- [ ] ShellCompletionGenAgent 针对特定程序生成 completions 脚本.
- [x] 一次请求生成多个结果.
- [x] 使用 ratatui 显示 tui 界面(嵌入在原来的 shell 之中, 非新开一个屏幕), 在交互结束之后回到原来的光标位置.
- [ ] 显示 markdown 内容.
- [x] 添加根据上次输入命令自动修正命令, 和 `howlto -- git push -up origin main` -> `git push -u origin main` 这样的命令自动修正功能.
- [ ] howlto 直接执行进入交互模式.
- [x] indicatif 实时显示模型输出在一行.
- [ ] url 网页内容访问, 网页搜索.
- [ ] 缓存功能.
- [ ] 修改输入流内容功能 (单独 profile), 比如: `cat a.c | howlto -i convert to python code` 直接输出 python 纯代码.
- [ ] 命令生成之后回车直接进入 shell 输入框 (需要 shell 集成脚本).
- [ ] 自动识别提示词, 对应使用不同的 agent 和 profile: chat / command / explain.
  - [ ] 并提供参数强行指定某些模式.
- [ ] windows cmd / powershell / pwsh 测试.
  - [ ] 适配非 `--help` 获取帮助的方法.
  - [ ] windows 下没有 `man`, 寻找替代实现.
- [ ] gemini api.
- [x] 测试 gpt-4.1-nano 模型 (结果目测不如 gpt-4o-mini).
- [ ] 描述选中的命令 (ShellCommandGenAgent).
- [ ] feat: 执行之后根据错误码和输出的内容进行分析, 重新更改 command.
- [ ] 插件(流程控制) / mcp
- [ ] 编写一些测试 prompts, 用于检测 profile 更改之后是否符合要求.
  - [ ] macos, linux, windows 等各平台下的效果
  - [ ] cmd, powershell, fish, nushell, bash 等各个 shell 下的命令正确性
- [ ] `-x` 参数直接执行第一个候选命令.
- [ ] translate agent (stdin / text file).
- [ ] help/man tool: 搜索特定的 pattern, 增加阅读的效率.
- [ ] shell 内置命令支持.
- [ ] 不同的 shell 的 execute 有不同的方式.
- [ ] 将任务根据难度分层, 对于最简单的任务, 对其不进行 check help.

## Issues

只有 macOS Apple Silicon 被测试运行过, 其他平台上的使用如遇问题欢迎提出 issues.

## Referencing

- ⭐[TheR1D/shell_gpt](https://github.com/TheR1D/shell_gpt): howlto is inspired by this repo.
- 🦆[szyazi/yazi](https://github.com/sxyazi/yazi): for it's shell integration.
- 🗯️[nvbn/thefuck](https://github.com/nvbn/thefuck): for it's shell integration and fixing command ideas.
- 📒[tldr-pages/tldr](https://github.com/tldr-pages/tldr): for it's TL;DR content.

## Enjoy :)
