# 0007 diff 视图改用 gpui-component 的 `Diff` 组件

日期:2026-11-04 · 状态:已采纳(取代 0005 里“不能替换现有 diff 视图”的结论)

## 背景

自己写的 diff 视图(`uniform_list` + 每行一个 `StyledText`)做不到编辑器级的体验:
不能用鼠标拖选文字,长行不能横向滚动,并排视图左右两侧也没有共用的滚动偏移。
逐项补齐等于重写一个文本选区与滚动系统。

调研过的方案:

- gpui-component 的 `Editor`(只读):选择、滚动现成,但整行底色(`Fill` 只铺文字宽度)、
  旧 / 新两列行号做不出来,diff 的核心信息无处安放;
- Zed 的 `editor` / multibuffer:GPL-3.0 且与 Zed 工作区深度耦合,不适合当依赖;
- gpui-component 的 `diff::Diff`:已在依赖里,0005 写作时低估了它。它自带虚拟列表、并排 / 统一、
  拖选复制、横向滚动、行号、词级差异,并且开放了我们需要的接口。

## 决定

diff 区域改用 `gpui::component::diff::{Diff, DiffState}`。

- gitcore 新增 `patch::display_patch`(把 `FileDiff` 还原成 git 补丁文本,供组件解析)与
  `highlight::highlight_source`(按路径给一段源码做 syntect 高亮,Vue 仍然支持);
  组件的 `with_syntax_highlighter` 注入这个函数,颜色仍由 `colors::syntax_color` 按亮暗主题决定;
- 行级暂存:监听 `DiffEvent::SelectionChanged`,`diff_stage::lines_in_range` 把组件的行范围
  换算成 `(hunk, 行)`,头部的“暂存选中的 N 行”按钮与 `ApplySelection` 命令沿用;
- hunk 级暂存:用组件的行号栏标记(`DiffGutterMarker`)在每个 hunk 第一条改动行旁放按钮;
- 复制:组件自带 ⌘C;右键“复制”走现有 `CopyDiffLines` 动作。

## 代价

- 没有 minimap(组件自带滚动条);
- hunk 标记位只放得下一个按钮,只保留暂存 / 取消暂存;“丢弃整个 hunk”改为选中行后用“丢弃选中行”;
- 配色改用组件的主题色,不再是自定义的浅粉 / 浅绿;
- 语法高亮由“从完整文件开头解析”退回“按 hunk 的一侧解析”:hunk 起点落在多行注释、
  模板字符串或 Vue 的 `<template>` / `<script>` 中间时,高亮可能不准。
  `app` 里按完整文件算高亮的后台任务(`highlight_diff`)现在不再被界面使用,后续清理;
- 文本选区由组件实现,我们无法改它的行为(如拖到边缘自动滚动取决于组件)。
