# Todo

## ✅ Done

- [x] 简单调用, e.g: `howlto create a rust project?`.
- [x] 交互式选择, 修改, 直接执行或者复制输出的命令.
- [x] 自动读取命令的 `--help` / `man` / `tldr`.
- [x] 类似 `thefuck` 一样自动修复上一个在 shell 中执行的命令.
- [x] 添加 `thefuck` 帮助 tool.
- [x] 根据帮助文档识别子命令的帮助文档, 并根据子命令生成可用的命令.
- [x] 一次请求生成多个结果.
- [x] 使用 ratatui 显示 tui 界面(嵌入在原来的 shell 之中, 非新开一个屏幕), 在交互结束之后回到原来的光标位置.
- [x] 添加根据上次输入命令自动修正命令, 和 `howlto -- git push -up origin main` -> `git push -u origin main` 这样的命令自动修正功能.
- [x] 测试 gpt-4.1-nano 模型 (结果目测不如 gpt-4o-mini).
- [x] indicatif 实时显示模型输出在一行.
- [x] 减小构建大小.

## 👨‍💻 Progressing

- [ ] 给每个输出的命令一个简短的 description (finish tool 要改名).
- [ ] 在 Windows shell 下可能会检测到第一个 Enter, 是由于执行命令产生的, 应该忽略.
- [ ] 多段对话功能, 在一次执行能多段对话, 并且在同一个 shell 中连续执行多次能够接上上面的对话内容 (detect_shell 中保存 shell 的 pid 作为键).
- [ ] ShellCompletionGenAgent 针对特定程序生成 completions 脚本.
- [ ] 显示 markdown 内容.
- [ ] howlto 直接执行进入交互模式.
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
- [ ] 描述选中的命令 (ShellCommandGenAgent).
- [ ] feat: 执行之后根据错误码和输出的内容进行分析, 重新更改 command.
- [ ] 插件(流程控制) / mcp
- [ ] 编写一些测试 prompts, 用于检测 profile 更改之后是否符合要求.
  - [ ] macos, linux, windows 等各平台下的效果
  - [ ] cmd, powershell, fish, nushell, bash 等各个 shell 下的命令正确性
- [ ] `-x` 参数直接执行第一个候选命令.
- [ ] translate agent (stdin / text file).
- [ ] help/man tool: 搜索特定的 pattern, 增加阅读的效率.
- [ ] shell 内置命令 (builtin) 支持, 也许可以通过获取各个 shell 的帮助页面网页内容实现.
- [ ] 不同的 shell 的 execute 有不同的方式.
- [ ] 将任务根据难度分层, 对于最简单的任务, 对其不进行 check help.
- [ ] linux 下补充发行版在 os 字段.
- [ ] shell 集成快捷键, 绑定某个快捷键将输入内容描述替换成命令 (强制 command gen profile).
- [ ] tree 工具, 列出本地文件结构命令.
- [ ] project status 工具, 给 agent 提供当前文件夹下的项目状态, 比如 rust editon, git branch 等, 需要支持各种语言.
- [ ] 收藏夹功能, 收藏某个命令, 活着收藏某个命令的思路, 让 Agent 根据收藏项及其描述来生成命令.

## ❌ Cancelled

- shell 集成, 自动补全, 自动获取上一个命令的输出(这点比较难做到) (使用一个参数或者 shell 命令类似 `last_output | howlto ...` 控制这点).
