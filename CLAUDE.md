# Gitlane 项目规则

Rust + GPUI 的 macOS Git 客户端,个人自用。需求来源是 `docs/plan.md`,进度以 `docs/roadmap.md` 为准。
本文件是所有改动都要遵守的规则;细节与示例在 `docs/development.md`。

## 1. 工作流程

1. **动手前**:先看 `docs/roadmap.md` 确认要做的项和验收标准;涉及架构取舍的,先读 `docs/architecture.md` 与 `docs/decisions/`。
2. **改动后必须通过**(提交前一次不落):
   - `cargo fmt --all`
   - `cargo clippy --workspace --all-targets` —— **零警告**
   - `cargo test --workspace` —— 全部通过
3. **重启开发实例**:每轮修改结束时,Stop hook(`.claude/settings.json`)会自动执行
   `scripts/dev-restart.sh --if-changed`:源码有变化就构建并重启 `target/debug/gitlane`,恢复上次打开的仓库。
   - 构建失败时旧实例不动,hook 返回错误,必须当轮修好;
   - 需要手动重启时运行 `scripts/dev-restart.sh`(可带仓库路径);
   - 运行日志 `target/dev-run.log`,构建日志 `target/dev-build.log`。
4. **同步文档**:功能完成或范围变化时更新 `docs/roadmap.md` 的状态;改了分层、线程模型、撤销方式时更新
   `docs/architecture.md`;做了新的取舍写一篇 `docs/decisions/NNNN-*.md`;有新的实测数据写进 `docs/testing.md`。
5. **如实汇报**:没验证的不说“已完成”;只能手工验证的(输入法、像素、帧率)标 ☑️ 并写进 testing.md 的手工清单。
6. **提交并推送**:每轮修改完成、第 2 步检查全部通过后,必须立即提交并 `git push`,不必等用户要求。
   提交信息用中文,首行说明“做了什么”,不超过 50 字;只提交本轮相关的文件,不顺带提交无关改动。
   检查没过不提交;推送失败要如实汇报原因,不得 force push。

## 2. 架构边界(不可违反)

- 依赖方向:`gitcore` → `graph` → `app` → `ui`,下层不得引用上层;`gitcore`、`graph` **不得依赖 GPUI**。
- **UI 线程不调用 git、不做重计算**。读写都经 `RepoModel`,在 `cx.background_spawn` 里执行,结果带代数写回,过期结果丢弃。
- **读写分路**:读用 git2(`gitcore::Repo`),写一律用系统 git(`gitcore::Git`),不要用 git2 写仓库。
- **所有写操作走命令层**:`app::Command` + `command::execute`。视图只调用 `RepoView::run(Command)`,不得直接调用 `gitcore` 的写方法。
- **撤销**:会移动引用或丢弃内容的命令必须生成撤销记录(`record_refs` / `record_files`),并为它写执行 + 撤销的测试。
- **危险操作先确认**:force push、hard reset、丢弃改动、删除分支,用 `dialogs::confirm` 列出将受影响的提交或文件。

## 3. 新增功能的标准做法

| 要加的东西 | 步骤 |
|---|---|
| 写操作 | `gitcore/src/ops.rs` 加方法 → `Command` 加变体、`label()`、撤销方式 → `gitcore/tests/repo_ops.rs` 加执行与撤销测试 → UI 入口 |
| 读数据 | `gitcore/src/repo.rs` 或 `diff.rs` 返回 `Send + Clone` 的纯数据类型 → `RepoModel` 后台加载 |
| 动作 / 快捷键 | `ui/src/actions.rs`:`actions!` → `bind_keys` → `palette_entries` → 视图上 `.on_action` |
| 新图标 | 在 `ui/src/assets.rs` 的 `icon_assets!` 登记,`Icon::from(gpui::assets::IconName::Xxx)` |
| 设置项 | `app/src/settings.rs`,带 `#[serde(default)]`,旧设置文件必须能读 |

## 4. 代码风格

- 注释、界面文案、错误信息、日志一律中文;注释写“为什么”,密度与周围代码一致,不写复述代码的注释。
- 命名沿用现有习惯:视图方法 `render_xxx`,动作处理 `on_xxx`,后台加载函数 `load_xxx`。
- 错误用 `gitcore::Error` / `Result`,不 `unwrap()` 可能失败的 IO 与 git 调用;仅测试与“逻辑上不可能”处可 `expect("原因")`。
- 渲染 helper 返回 `impl IntoElement + use<>`;`cx.theme()` 之后还要用 `cx` 时先 `.clone()`。
- 颜色、间距用 gpui-component 主题(`cx.theme()`)和 `ui/src/colors.rs`,不在视图里写裸色值;亮暗主题都要看得清。
- 大列表一律虚拟滚动(`uniform_list`),行高固定;每行渲染只做 O(1) 工作,需要预计算的放到缓存(参考 `DiffCache`)。
  任何 `cx.notify()` 都会重绘整个窗口,所以**任何可能超过几十项的列表都不能直接 `.children(全部)`**。
- 行内既有复选框/按钮又整行可点时,只让一处处理点击(子控件只显示,或 `cx.stop_propagation()`),否则会切换两次。

## 5. 测试规则

- 测试只用 `tempfile` 临时仓库,**不得对真实仓库做写操作**,也不得读写 `~/Library/Application Support/Gitlane`。
- UI 测试用 `Workspace::new(Settings::default(), true, cx)`(不持久化、不监听文件),窗口用 `gpui::open_window`。
- 自定义元素要被测试点击,链尾加 `.test_support()` 并给稳定的 `.id(...)`。
- `crates/ui` 的测试模块不要 `use super::*`(会导入 gpui 的 `test` 宏遮住 `#[test]`),显式列出要用的名字。
- 修 bug 先写能复现的测试。性能相关改动附上 `bench_layout` / `bench_load` 前后对比。

## 6. 依赖与工具链

- `gpui = { package = "gpui-kit", version = "=0.9.0" }`、工具链 1.99.0 都是**固定**的,升级要单独提出并走 `docs/decisions/0002-pin-gpui.md` 的流程。
- 不随手 `cargo update`;lockfile 无解时删掉 `Cargo.lock` 重新生成。
- 新增依赖前先确认现有依赖(gpui-component、similar、notify、futures)不能满足,并在提交说明里写明理由。

## 7. 常用命令

```sh
scripts/dev-restart.sh [仓库…]                                 # 构建并重启开发实例
cargo test --workspace                                         # 全部测试
cargo clippy --workspace --all-targets && cargo fmt --all
./scripts/bundle.sh                                            # dist/Gitlane.app
python3 scripts/gen_big_repo.py /tmp/big 50000                 # 大仓库测试数据
cargo run -p graph --release --example bench_layout -- 50000   # 布局基准
cargo run -p gitcore --release --example bench_load -- <仓库>  # 读路径基准
```
