# 0005 diff 语法高亮直接用 syntect

日期:2026-11-04 · 状态:已采纳

## 背景

diff 需要语法高亮。日常主要看 Rust、TypeScript 与 Vue 前端仓库,Vue 单文件组件必须能着色。

gpui-component 0.9.0 自带两套高亮:

- `tree-sitter` 特性:`SyntaxHighlighter` 公开可用,但每种语言单独一个 grammar 特性,**没有 Vue**,全开体积几十 MB;
- `syntect` 特性:约两百种语言、几 MB,含 Vue,但给任意文本做高亮的函数是 `pub(crate)`,
  对外只有编辑器输入框、Markdown 代码块和它自己的 `Diff` 组件能用。
  它的 `Diff` 组件不支持我们已有的行级暂存、hunk 按钮与 minimap,不能替换现有 diff 视图。

## 决定

直接依赖 `syntect` 与 `two-face`(bat 的语法集),版本与特性和 gpui-component 的可选依赖一致
(`regex-fancy`,纯 Rust,不引入 C 编译)。

- 高亮放在 `gitcore::highlight`,不依赖 GPUI,输出与主题无关的高亮名(`keyword`、`string`…,
  命名沿用 gpui-component 主题),颜色在 `ui/src/colors.rs` 按亮暗主题映射;
- 尽量从完整文件开头解析,跨行注释、模板字符串、Vue 的 `<script>` 段才能着色正确;
  从开头到最后一个改动行超过 512 KB、或文件内容与 diff 对不上时,退回只解析 hunk;
- 高亮在后台算,diff 先显示、高亮后到(见 architecture.md 的线程模型)。

## 代价

- syntect 约每秒 2 万行,10,000 行文件末尾的改动要约 0.6 秒才出高亮(实测见 testing.md);
  换 Oniguruma 只快约 13%,不值得引入 C 依赖;
- 正则高亮不理解语义,个别语言的少数 token 颜色会与编辑器不同。
