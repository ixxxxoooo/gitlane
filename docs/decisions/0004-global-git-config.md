# 0004 全局 git 配置在设置窗口里读写,不走命令层

日期:2026-11-04 · 状态:已采纳

## 背景

常见的 Git 客户端都能在设置里改提交身份、pull 方式、签名等。这些值存在全局 `~/.gitconfig`,
不属于某个仓库;而项目规则要求“所有写操作走 `app::Command` + 撤销记录”“读用 git2、写用系统 git”。

## 决定

- 读写都用系统 `git config --global`(`gitcore::config::ConfigFile`),**读也不用 git2**:
  这样 include、XDG 路径(`~/.config/git/config`)、转义与布尔写法(yes / on / 1)的解释与用户命令行完全一致,
  也只有一条代码路径需要测试。
- 不进 `Command` 枚举与撤销栈:`Command` 的语义是“在某个仓库里执行”,撤销栈也按仓库存放;
  配置项本身就能在设置窗口里改回去,值也直接显示在界面上。
- 仍守住分层:设置窗口只调用 `Workspace::load_git_config` / `set_git_config`,
  由 `Workspace` 在后台执行器里调用 gitcore,UI 线程不调用 git。
- 只管全局配置,不做按仓库覆盖;仓库的 `.git/config` 里设置的值仍然优先(界面上写明)。
- pull 方式切换时同时清掉另一个键(`pull.rebase` 与 `pull.ff=only` 互相影响),避免残留配置让行为与界面不符。
- 文本项(用户名、邮箱、默认分支)失焦或回车时才写入,避免每次按键都起一个 git 进程。

## 测试

`ConfigFile::at(临时文件)` 等价于 `git config --file`;单测与 UI 测试都指向临时文件。
ephemeral 模式的 `Workspace` 没有配置文件,永远不会碰到用户真实的 `~/.gitconfig`。
