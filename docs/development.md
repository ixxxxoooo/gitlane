# 开发指南

## 环境

| 依赖 | 版本 | 说明 |
|---|---|---|
| macOS | 13+ | GPUI 用 Metal 渲染 |
| Xcode 与命令行工具 | 最新 | 提供 Metal 编译器与系统 git(`xcode-select --install`) |
| Rust | 1.99.0 | 由 `rust-toolchain.toml` 固定,rustup 会自动切换。gpui-pre 0.3.11 需要 ≥ 1.93 |
| git | ≥ 2.30 | 写路径直接调用系统 git,启动时检测版本 |

## 常用命令

```sh
scripts/dev-restart.sh [仓库…]              # 构建并重启开发实例(Claude Code 每轮修改后自动执行)
cargo run -p ui -- /path/to/repo           # 前台运行,可带多个仓库路径
cargo run -p ui                             # 恢复上次打开的仓库
cargo test --workspace                      # 全部测试(含无头 UI 测试)
cargo clippy --workspace --all-targets      # 保持零警告
cargo fmt --all
./scripts/bundle.sh [--dmg]                 # release 构建并生成 dist/Gitlane.app(--dmg 另生成安装包)
./scripts/publish.sh                        # 发版:打包、打 tag、推送、创建 GitHub Release
```

日志:`RUST_LOG=debug cargo run -p ui -- …`,会打印每条执行的 git 命令和加载耗时。

## 测试数据

```sh
python3 scripts/gen_big_repo.py /tmp/big-repo 50000          # 约 5 万提交、带分支与合并、每千个提交一个标签
cargo run -p graph --release --example bench_layout -- 50000 # 泳道布局基准
```

不要拿日常仓库做写操作测试,集成测试一律使用 `tempfile` 临时仓库。

## 目录

```
crates/
  gitcore/   读写 git、diff、补丁、撤销快照、文件监听(不依赖 GPUI)
  graph/     泳道布局(纯函数)+ examples/bench_layout.rs
  app/       RepoModel / Workspace Entity、命令层、设置
  ui/        GPUI 视图,二进制 gitlane
docs/        方案、架构、进度、决策记录、测试清单
scripts/     打包与测试数据脚本
```

## 新增一个写操作

1. `gitcore/src/ops.rs`:在 `impl Git` 里加调用系统 git 的方法;
2. `app/src/command.rs`:`Command` 加变体,补 `label()`,在 `execute` 里决定撤销方式
   (`record_refs` + 合适的 `ResetMode`,或 `record_files`,或不记录);
   只改索引/工作区的命令要加进 `touches_history()` 的排除列表;
3. `ui`:按钮或菜单项调用 `RepoView::run(Command::…)`;危险操作先用 `dialogs::confirm` 列出影响范围;
4. 在 `gitcore/tests/repo_ops.rs` 加执行与撤销的测试。

## 发版与应用内更新

1. 改根 `Cargo.toml` 的 `[workspace.package] version`,按常规提交并推送(检查全部通过)。
2. 运行 `scripts/publish.sh`(需要已登录的 `gh`):打包 dmg → 生成 `dist/appcast.json` → 打 `v<版本>` tag 并推送
   → 创建 Release 并上传 dmg 与 appcast。也可以只推 tag,由 `release.yml` 在云端构建上传。
3. 已安装的应用启动时和之后每 3 小时检查 `releases/latest/download/appcast.json`,发现新版本后台下载校验,
   通知里点“重启更新”完成替换。设置里可关闭自动检查,菜单“检查更新…”可手动触发。

实现:`app/src/update/`(检查、下载、校验、替换,可脱离 GPUI 测试)与 `ui/src/updater.rs`(状态与通知)。
取舍见 [decisions/0006-self-update.md](decisions/0006-self-update.md)。开发构建(`cargo run`、开发实例)构建号为 0,
不会自动检查,也不是从 `.app` 运行,所以不能自动替换。

## 新增一个动作 / 快捷键

`ui/src/actions.rs`:在 `actions!` 里加名字 → `bind_keys` 绑定(仓库级动作用 `REPO_CONTEXT`)→
`palette_entries` 加命令面板条目 → 在 `RepoView::render` 或 `GitlaneWindow::render` 上 `.on_action(...)`。

## GPUI 使用约定与踩过的坑

- **依赖固定**:`gpui = { package = "gpui-kit", version = "=0.9.0" }`,内部固定 gpui-pre 0.3.11。
  代码里统一 `use gpui::…`,组件在 `gpui::component::…`,图标资源在 `gpui::assets::…`。升级单独排期,见
  [decisions/0002-pin-gpui.md](decisions/0002-pin-gpui.md)。
- **不要随手 `cargo update`**:gpui-kit 的可选依赖 gpui-fast 精确固定了 taffy 等版本,
  Cargo 解析 lockfile 时也会把它算进去。出现 `failed to select a version for taffy` 时,
  删掉 `Cargo.lock` 重新 `cargo generate-lockfile`,不要逐个 `--precise`。
- **Rust 2024 的返回类型捕获**:`fn render_xxx(&mut self, cx: &mut Context<Self>) -> impl IntoElement`
  会捕获 `self` 与 `cx` 的借用,导致在 `render` 里无法连续调用多个 helper。helper 一律写成
  `-> impl IntoElement + use<>`。
- **主题借用**:`cx.theme()` 借用了 `cx`,之后还要调 `cx.listener(...)` 时先 `.clone()`。
- **图标**:gpui-component 的 `IconName` 只包含默认图标集;要用其它 Lucide 图标(如 `GitBranch`、`Tag`),
  在 `ui/src/assets.rs` 的 `icon_assets!` 列表里登记,并用 `Icon::from(gpui::assets::IconName::Xxx)`。
- **窗口入口**:必须用 `gpui::open_window`(会挂载 Root,对话框与通知依赖它),测试里也一样。
- **菜单与动作**:右键菜单项通过 `window.dispatch_action` 把参数送回视图(见 `sidebar.rs` 的 `RefAction`),
  不在菜单闭包里持有视图引用。
- **后台任务**:前台用 `cx.spawn(async move |this, cx| …)`,后台用 `cx.background_spawn(async move { … })`;
  耗时结果写回前检查代数,过期的直接丢弃。
- **无头 UI 测试**:`[dev-dependencies] gpui = { workspace = true, features = ["test-support"] }`,
  `#[gpui::test]` + `gpui::test::{TestWindowExt, TestAppContextExt}`。自定义元素要被 `window.find/click`
  找到,需在链尾加 `.test_support()`(非测试构建下是空操作)。工作区用 `Workspace::new(Settings::default(), true, cx)`,
  不会写真实设置文件,也不启动文件监听。
