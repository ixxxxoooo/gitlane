//! 上游 `Diff.*` 文案的中文版本。
//!
//! @author ixxxxoooo

/// 按上游的键取中文文案,未知的键原样返回(便于发现漏译)。
pub(crate) fn text(key: &'static str) -> &'static str {
    match key {
        "Diff.Viewer" => "%{name} 的改动",
        "Diff.ViewerFiles" => "%{count} 个文件的改动",
        "Diff.HiddenLines" => "%{count} 行未显示",
        "Diff.ExpandBelow" => "向下展开 %{count} 行",
        "Diff.ExpandAbove" => "向上展开 %{count} 行",
        "Diff.UnchangedLines" => "%{count} 行未改动",
        "Diff.ExpandLines" => "展开 %{count} 行",
        "Diff.AcceptedCurrent" => "已采用当前改动",
        "Diff.AcceptedIncoming" => "已采用传入改动",
        "Diff.AcceptedBoth" => "已采用两边",
        "Diff.UndoResolution" => "撤销",
        "Diff.CurrentChange" => "当前改动",
        "Diff.BaseChange" => "共同祖先",
        "Diff.IncomingChange" => "传入改动",
        "Diff.AcceptCurrent" => "采用当前",
        "Diff.AcceptIncoming" => "采用传入",
        "Diff.AcceptBoth" => "采用两边",
        "Diff.AddedFile" => "新增",
        "Diff.DeletedFile" => "删除",
        "Diff.RenamedFile" => "重命名",
        "Diff.CopiedFile" => "复制",
        "Diff.ConflictedFile" => "冲突",
        "Diff.ExpandFile" => "展开 %{name}",
        "Diff.CollapseFile" => "折叠 %{name}",
        "Diff.NoChanges" => "没有改动",
        "Diff.BinaryChanges" => "二进制文件,不显示内容",
        "Diff.NoTextChanges" => "没有文本改动",
        "Diff.EmptyFile" => "空文件",
        "Diff.Original" => "旧版本",
        "Diff.Modified" => "新版本",
        "Diff.AddAnnotation" => "在第 %{line} 行添加评论",
        "Diff.NoNewline" => "文件末尾没有换行",
        "Diff.RemovedLine" => "%{side}第 %{line} 行,已删除",
        "Diff.AddedLine" => "%{side}第 %{line} 行,新增",
        "Diff.SourceLine" => "%{side}第 %{line} 行",
        "Diff.SelectLine" => "选中%{side}第 %{line} 行",
        other => other,
    }
}
