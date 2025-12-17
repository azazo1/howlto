# howlto

一个命令行工具, 帮你快速找到想要使用的命令.

Fast 🚀, cheap 🤩, and low-spec 🗿 -- driven perfectly by `gpt-4o-mini`.

## Demo

<video src="assets/demo.mp4" title="Demo" autoplay muted loop></video>

## 安装

```shell
cargo install --git https://github.com/azazo1/howlto.git
```

或者查看 Releases 中预编译的可执行文件.

## 配置

默认配置目录: `~/.config/howlto/config.toml` (Windows 下 `~` 为 `%USERPROFILE%`).

先启动一次 `howlto` 创建配置文件 (可以忽略此次报错),
修改配置文件中的 `llm_api_key` 和 `llm_base_url` (目前只支持 openai 格式, 需要 `/v1` 后缀),
然后就能正常使用了.

## 使用

直接像普通的英语句子一样提问:

```shell
howlto upgrade all bun packages
```

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

## 🗺️ Route

- [x] 简单调用, e.g: `howlto create a rust project?`.
- [x] 交互式选择, 修改, 直接执行或者复制输出的命令.
- [x] 自动读取命令的 `--help` / `man` / `tldr`.
- [ ] 类似 `thefuck` 一样自动修复上一个在 shell 中执行的命令.
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

## Issues

只有 macOS Apple Silicon 被测试运行过, 其他平台上的使用如遇问题欢迎提出 issues.

## Enjoy :)
