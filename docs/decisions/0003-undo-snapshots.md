# 0003 撤销基于引用快照与文件快照

日期:2026-11-04 · 状态:已采纳

## 背景

方案要求 checkout、commit、discard、删除分支、reset 等操作都能一键撤销与重做,且不能丢数据。

## 决定

所有写操作经命令层执行,执行前后各拍一次快照,撤销 = 恢复“前”,重做 = 恢复“后”。

- **引用快照**(`RefSnapshot`):HEAD 指向(分支 / 分离 / 未出生)+ 全部 `refs/heads/*`、`refs/tags/*`。
  恢复顺序:先改未检出的引用 → 处理 HEAD(同一分支用 `reset`,换分支用 `checkout`)→ 再改原先检出的分支。
  同一分支移动时的 reset 方式随命令类型记录:commit 用 soft(改动回到暂存区),reset 用原方式,
  cherry-pick / revert / merge / pull 用 keep(保留不冲突的本地改动)。
- **工作区改动**:hard reset 前用 `git stash create` 生成一个不改变工作区的 stash 提交,撤销时
  `reset --hard` 回原提交后 `git stash apply --index` 应用回来。
- **文件快照**(`FileSnapshot`):丢弃前把文件当前内容写入对象库(blob),撤销时写回;文件原本不存在则删除。
- 前后快照相同的记录不入栈;撤销或重做失败时记录放回原栈。
- 撤销栈只在内存中,每仓库上限 100 条。

## 不覆盖

stage / unstage(本身就能反向操作)、stash、fetch、push(远程的变化无法本地撤销)。

## 风险与对策

每类操作在 `gitcore/tests/repo_ops.rs` 单独写撤销测试;hard reset、force push、丢弃改动执行前弹确认框并列出影响范围。
