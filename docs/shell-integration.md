# Shell 集成

todo

## init

### bash

- 无法实现选择的命令放置到输入框中.

### nushell

```nushell
howlto --init | save -f ~/.zoxide.nu
source ~/.zoxide.nu
```

## deinit

<!-- todo 介绍: 进行 --init 之后会覆盖 howlto 可执行文件, 如果需要调用原来的可执行文件, 使用 command howlto 等方法 -->