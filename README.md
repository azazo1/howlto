# howto

一个命令行工具, 帮你快速找到想要使用的命令.

## 🗺️ Route

- [x] 简单调用, e.g: `howlto create a rust project?`.
- [ ] 交互式选择, 修改, 直接执行或者复制输出的命令.
- [x] 自动读取命令的 `--help` / `man` / `tldr`.
- [ ] 类似 `fuck` 一样自动修复上一个命令.
- [x] 根据帮助文档识别子命令的帮助文档, 并根据子命令生成可用的命令.
- [ ] 多段对话功能, 在一次执行能多段对话, 并且在同一个 shell 中连续执行多次能够接上上面的对话内容.
- [ ] shell 集成, 自动补全, 自动获取上一个命令的输出 (使用一个参数或者 shell 命令类似 `last_output | howlto ...` 控制这点).
- [ ] 生成 completions 脚本.
- [x] 一次请求生成多个结果.
- [x] 使用 ratatui 显示 tui 界面(嵌入在原来的 shell 之中, 非新开一个屏幕), 在交互结束之后回到原来的光标位置.
- [ ] 显示 markdown 内容.
- [x] 添加根据上次输入命令自动修正命令, 和 `howlto -- git push -up origin main` -> `git push -u origin main` 这样的命令自动修正功能.
- [ ] howlto 直接执行进入交互模式.
- [x] indicatif 实时显示模型输出在一行.
- [ ] url 网页内容访问, 网页搜索.
