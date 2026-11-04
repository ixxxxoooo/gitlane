# 排期与进度

按 [plan.md](plan.md) 的“实现顺序与验收”推进,中途不单独发版。每完成一项在这里打勾,并写明验证方式。

图例:✅ 已完成并有自动化测试 · ☑️ 已完成、需手工验证 · 🚧 部分完成 · ⬜ 未开始

## 第 1 步 技术验证(约 2 周)

验收:滚动流畅,中文输入法可用。不达标就重新比较方案,不往下做。

| 项目 | 状态 | 说明 |
|---|---|---|
| GPUI + gpui-component 在本机构建运行 | ✅ | gpui-kit 0.9.0 / gpui-pre 0.3.11,Rust 1.99 |
| 提交图最小原型(虚拟滚动 + 泳道) | ✅ | 已直接做成正式实现 |
| diff 最小原型 | ✅ | 同上 |
| 5 万提交仓库加载 | ✅ | 52,499 提交 480–880 ms,布局 1.9 ms;满足“1 秒内可操作” |
| 5 万提交滚动 60 fps | ☑️ | 待在真机上用 `scripts/gen_big_repo.py` 生成的仓库实测,见 testing.md |
| 中文输入法(提交框、搜索框、分支名输入框) | ☑️ | 输入框使用 gpui-component 的 Input/Textarea(实现了 GPUI 的 IME 接口),候选框位置与组合态需手工验证 |

## 第 2 步 主线功能(约 6 周)

验收:能替代自己日常的提交与同步操作。

| 模块 | 功能 | 状态 |
|---|---|---|
| 仓库与刷新 | 多仓库标签、打开本地仓库、恢复上次打开的仓库、最近列表 | ✅ |
| | 扫描目录(⌘⇧O):查找 4 层内的 Git 仓库,勾选后批量添加,已打开的标灰 | ✅ |
| | 文件监听实时刷新(防抖 300 ms,区分工作区 / .git) | ✅ |
| 提交图 | 分支泳道、分支与标签徽章、HEAD 标记、合并提交空心点 | ✅ |
| | 虚拟滚动、固定的 Working changes 行、键盘上下选择 | ✅ |
| | 按信息 / 作者 / SHA 搜索,回车跳到下一个结果 | ✅ |
| 提交详情与 diff | 作者、时间、SHA(点击复制)、完整信息、改动文件列表 | ✅ |
| | 统一与并排视图、显示完整文件并折叠未改动区域、鼠标拖选复制、不换行(并排各占半屏、横向滚动同步)、大文件摘要 + 仍然显示。fork 自 gpui-component 的 diff 模块(`crates/diffview`,见 decisions/0008);待做:上一处 / 下一处改动、滚动条改动标记、配色 | ✅(拖选与滚动待手工验证) |
| | 语法高亮 | ✅ syntect + bat 语法集(含 Vue / TypeScript),从完整文件解析;颜色亮色贴近 VS Code Light+、暗色对应 Dark+;配色是否合适需手工看(见 testing.md),选型见 [0005](decisions/0005-syntax-highlight.md) |
| 暂存与提交 | 文件、hunk、单行三级暂存;取消暂存;丢弃(带确认,可撤销) | ✅ |
| | 提交信息输入、amend(带出上次信息)、⌘↵ 提交 | ✅ |
| | 提交签名 | 🚧 遵循用户的 `commit.gpgsign` 配置;界面上的强制签名开关未做(`CommitOptions::sign` 已支持) |
| 分支与同步 | 创建、切换、重命名、删除、按前缀树状分组、领先/落后 | ✅ |
| | fetch、pull、push(无上游时自动 `-u`)、force push(`--force-with-lease`,确认框列出远程将丢失的提交数) | ✅ |
| | 联网操作可取消 | ✅ |

## 第 3 步 差异化功能(约 7 周)

验收:常见的 rebase 与冲突场景能完成,且能撤销。

| 模块 | 状态 | 说明 |
|---|---|---|
| 撤销与重做 | ✅ | commit、checkout、discard、删除分支、reset(含 hard)、新建/重命名分支、cherry-pick、revert、merge、pull、标签 |
| 交互式 rebase | ⬜ | 拖拽重排、squash、fixup、edit、drop,执行前预览。计划:界面生成 todo 列表 → `GIT_SEQUENCE_EDITOR` 写入 → `git rebase -i`,撤销复用引用快照 |
| 冲突编辑器 | 🚧 | 已有:进行中操作横幅 + 中止按钮、冲突文件标记。未做:三方并排编辑器。可评估 gpui-component 自带的 `diff::conflict` |

## 第 4 步 其余 Git 能力(约 4 周)

| 功能 | 状态 |
|---|---|
| stash(push / apply / pop / drop) | ✅ |
| reset soft / mixed / hard(hard 前列出受影响的提交与文件) | ✅ |
| cherry-pick、revert、merge | ✅ |
| tag 新建 / 删除 | ✅ |
| 命令面板(⌘K,模糊搜索全部动作) | ✅ |
| blame | ⬜ |
| 文件历史 | ⬜ |
| reflog | ⬜ |
| bisect 界面 | ⬜ |
| worktrees | ⬜ |

## 第 5 步 打磨与打包(约 3 周)

| 项目 | 状态 |
|---|---|
| 本机打包 `.app`(`scripts/bundle.sh`) | ✅ |
| 应用内更新(检查 / 后台下载校验 / 重启替换)与发版(`scripts/publish.sh`、tag 触发的 release 流水线、CI) | ☑️ 检查、比较、暂存、失败记录有自动化测试;真实下载与替换需发一次版手工验证,见 testing.md;选型见 [0006](decisions/0006-self-update.md) |
| App 图标 | ☑️ 提交图泳道图形,浅色 / 深色两版;打包用 `assets/icon/Gitlane.icns`,运行中 Dock 图标随主题切换(源文件与生成脚本在 `assets/icon/`) |
| 设置窗口(⌘,) | ✅ 通用(主题含跟随系统、恢复标签、自动 fetch 间隔)、Git(全局 user.name / user.email、pull 方式、commit.gpgsign、init.defaultBranch)、提交图与 Diff(提交上限、默认并排、上下文行数)、集成(终端应用,⌥⌘T 在终端中打开、⌥⌘R 在 Finder 中显示)。界面细节见 testing.md 手工清单 |
| 亮色 / 暗色主题切换并记住 | ✅ 另可在设置里选“跟随系统” |
| 面板宽度可拖动调整 | ✅ 拖动分隔条调整左右栏、双击恢复默认;宽度记在设置里,窗口缩放时保持不变(只有中间伸缩,窗口太窄时临时收窄) |
| 性能收尾:5 万提交时内存 248 MB 超出 150 MB 目标;首屏增量加载历史 | ⬜ 原因与候选方案见 testing.md“已定位的性能问题” |
| 中文输入与高分屏回归 | ☑️ 按 testing.md 的手工清单执行 |

## 已知限制

- 提交图默认最多加载 100,000 个提交(`Settings::history_limit`),超出部分不显示;
- rebase 进行中时只能中止,继续需要在命令行执行 `git rebase --continue`;
- 撤销栈只在内存中,关闭标签或退出应用后清空;
- 命令面板的结果列表暂不支持方向键选择,回车执行第一个匹配项。
