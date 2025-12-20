# Shell 集成

## init

> [!NOTE]
> 下面的操作执行之后可能要重启 shell 才能生效.

### bash

将下面这行命令放入 `~/.bashrc` 中.

```shell
eval "$(howlto --init)"
```

- 无法实现选择的命令放置到输入框中.

### zsh

将下面这行命令放入 `~/.zshrc` 中.

```shell
eval "$(howlto --init)"
```

### nushell

将下面这行放在 `$nu.env-path` 文件中.

```nushell
^howlto --init | save -f ~/.howlto.nu
```

再将下面这行放在 `$nu.config-path` 中.

```nushell
source ~/.howlto.nu
```

### cmd

安装 [clink](https://github.com/chrisant996/clink).

```cmd
howlto --init > %LOCALAPPDATA%/clink/howlto.lua
```

然后重启 cmd, 注意 clink inject 后才能生效.

- 无法实现选择的命令放置到输入框中.

## deinit

<!-- todo 介绍: 进行 --init 之后会覆盖 howlto 可执行文件, 如果需要调用原来的可执行文件, 使用 command howlto 等方法 -->