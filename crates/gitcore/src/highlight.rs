//! diff 的语法高亮:用 syntect(bat 的语法集,含 Vue / TypeScript)给每行标出高亮名。
//!
//! 只产出 “keyword”、“string” 这类与主题无关的名字,颜色由界面层按亮暗主题决定。
//! 尽量用完整的旧 / 新文件从头解析,这样跨行的注释、模板字符串、Vue 里的 `<script>` 都能着色正确;
//! 拿不到完整文件(或太大)时退回只解析 hunk 里的行。

use crate::diff::{FileDiff, LineKind};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

/// 反序列化语法集要几十毫秒,首次高亮时在后台线程加载一次。
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// 从文件开头解析到最后一个改动行的字节数上限,超过就只解析 hunk。
/// syntect 大约每秒 2 万行(约 0.7 MB),512 KB 时约 0.6 秒,都在后台线程。
pub const FULL_PARSE_MAX_BYTES: usize = 512 * 1024;
/// 超长的行(压缩过的 JS 等)不解析,正则在这种行上会非常慢。
const MAX_LINE_BYTES: usize = 4096;

/// 一段高亮:行内字节区间 + 高亮名。
pub type SyntaxSpan = (Range<usize>, &'static str);

/// 与 `FileDiff::hunks` 一一对应的每行高亮。
#[derive(Clone, Debug, Default)]
pub struct DiffSyntax {
    pub hunks: Vec<Vec<Vec<SyntaxSpan>>>,
}

impl DiffSyntax {
    pub fn line(&self, hunk: usize, line: usize) -> &[SyntaxSpan] {
        self.hunks
            .get(hunk)
            .and_then(|h| h.get(line))
            .map_or(&[], Vec::as_slice)
    }
}

/// 按文件名找语法,纯文本或不认识的返回 `None`。
fn find_syntax(path: &str) -> Option<&'static SyntaxReference> {
    let set = &*SYNTAXES;
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map(|(_, e)| e);
    ext.and_then(|e| set.find_syntax_by_extension(e))
        // Makefile、Dockerfile 这类按整个文件名识别
        .or_else(|| set.find_syntax_by_extension(name))
        .filter(|s| s.name != "Plain Text")
}

/// 给 diff 的每一行算高亮。`old` / `new` 是两侧完整文件内容(新增 / 删除的文件只有一侧)。
/// 不认识的语言返回 `None`。
pub fn highlight_diff(file: &FileDiff, old: Option<&str>, new: Option<&str>) -> Option<DiffSyntax> {
    let syntax =
        find_syntax(&file.path).or_else(|| file.old_path.as_deref().and_then(find_syntax))?;

    // 两侧各自需要的行号:删除行看旧文件,新增行与上下文看新文件
    let mut need_old = Vec::new();
    let mut need_new = Vec::new();
    for hunk in &file.hunks {
        for l in &hunk.lines {
            match (l.kind, l.old_no, l.new_no) {
                (LineKind::Deleted, Some(n), _) => need_old.push(n),
                (_, _, Some(n)) => need_new.push(n),
                _ => {}
            }
        }
    }
    let old_spans = old.and_then(|t| spans_of_file(syntax, t, &need_old));
    let new_spans = new.and_then(|t| spans_of_file(syntax, t, &need_new));

    let hunks = file
        .hunks
        .iter()
        .map(|hunk| {
            let side = |kind: LineKind| match kind {
                LineKind::Deleted => old_spans.as_ref(),
                _ => new_spans.as_ref(),
            };
            // 从完整文件取到的高亮;取不到的行(没有文件、行内容对不上)整块按 hunk 解析
            let mut out: Vec<Option<Vec<SyntaxSpan>>> = hunk
                .lines
                .iter()
                .map(|l| {
                    let no = if l.kind == LineKind::Deleted {
                        l.old_no
                    } else {
                        l.new_no
                    }?;
                    let (text, spans) = side(l.kind)?.get(&no)?;
                    (*text == l.text).then(|| spans.clone())
                })
                .collect();
            if out.iter().any(Option::is_none) {
                // 旧侧是上下文 + 删除行,新侧是上下文 + 新增行;上下文取新侧的结果
                for (skip, take) in [
                    (LineKind::Added, LineKind::Deleted),
                    (LineKind::Deleted, LineKind::Added),
                ] {
                    let idx: Vec<usize> = (0..hunk.lines.len())
                        .filter(|&i| hunk.lines[i].kind != skip)
                        .collect();
                    let texts = idx.iter().map(|&i| hunk.lines[i].text.as_str());
                    for (&i, spans) in idx.iter().zip(parse_lines(syntax, texts)) {
                        let kind = hunk.lines[i].kind;
                        let mine =
                            kind == take || (kind == LineKind::Context && take == LineKind::Added);
                        if mine && out[i].is_none() {
                            out[i] = Some(spans);
                        }
                    }
                }
            }
            out.into_iter().map(Option::unwrap_or_default).collect()
        })
        .collect();
    Some(DiffSyntax { hunks })
}

/// 给一段独立的源码文本算高亮(diff 视图组件按 hunk 的一侧调用),`path` 用来识别语言。
///
/// 返回按位置排好、互不重叠的 (字节区间, 高亮名),区间相对整段 `text`;不认识的语言或过长的文本返回空。
pub fn highlight_source(path: &str, text: &str) -> Vec<SyntaxSpan> {
    let Some(syntax) = find_syntax(path) else {
        return Vec::new();
    };
    if text.len() > FULL_PARSE_MAX_BYTES {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0;
    let lines = text.split('\n');
    for (line, spans) in lines.clone().zip(parse_lines(syntax, lines)) {
        out.extend(
            spans
                .into_iter()
                .map(|(r, n)| (start + r.start..start + r.end, n)),
        );
        start += line.len() + 1;
    }
    out
}

/// 从头解析整个文件,只保留 `need` 里的行(1 起)。解析到最后一个需要的行就停。
#[allow(clippy::type_complexity)]
fn spans_of_file<'a>(
    syntax: &SyntaxReference,
    text: &'a str,
    need: &[u32],
) -> Option<HashMap<u32, (&'a str, Vec<SyntaxSpan>)>> {
    let last = *need.iter().max()?;
    let prefix = text
        .split_inclusive('\n')
        .take(last as usize)
        .map(str::len)
        .sum::<usize>();
    if prefix > FULL_PARSE_MAX_BYTES {
        return None;
    }
    let wanted: std::collections::HashSet<u32> = need.iter().copied().collect();
    let mut out = HashMap::with_capacity(wanted.len());
    let mut parser = Parser::new(syntax);
    for (i, line) in text.split_inclusive('\n').enumerate() {
        let no = i as u32 + 1;
        if no > last {
            break;
        }
        let spans = parser.line(line);
        if wanted.contains(&no) {
            let content = line.strip_suffix('\n').unwrap_or(line);
            let content = content.strip_suffix('\r').unwrap_or(content);
            out.insert(no, (content, clamp(spans, content.len())));
        }
    }
    Some(out)
}

/// 把若干行当作连续文本解析,返回每行的高亮。
fn parse_lines<'a>(
    syntax: &SyntaxReference,
    lines: impl Iterator<Item = &'a str>,
) -> Vec<Vec<SyntaxSpan>> {
    let mut parser = Parser::new(syntax);
    let mut buf = String::new();
    lines
        .map(|l| {
            buf.clear();
            buf.push_str(l);
            buf.push('\n');
            clamp(parser.line(&buf), l.len())
        })
        .collect()
}

/// 去掉落在换行符上的部分。
fn clamp(spans: Vec<SyntaxSpan>, len: usize) -> Vec<SyntaxSpan> {
    spans
        .into_iter()
        .filter_map(|(r, n)| (r.start < len).then(|| (r.start..r.end.min(len), n)))
        .collect()
}

/// 逐行解析的状态:syntect 的解析栈 + scope 到高亮名的缓存。
struct Parser {
    state: ParseState,
    scopes: ScopeStack,
    names: HashMap<Scope, Option<&'static str>>,
}

impl Parser {
    fn new(syntax: &SyntaxReference) -> Self {
        Self {
            state: ParseState::new(syntax),
            scopes: ScopeStack::new(),
            names: HashMap::new(),
        }
    }

    /// `line` 带换行符。返回按位置排好、互不重叠的高亮。
    fn line(&mut self, line: &str) -> Vec<SyntaxSpan> {
        let mut out: Vec<SyntaxSpan> = Vec::new();
        if line.len() > MAX_LINE_BYTES {
            return out;
        }
        // 解析失败的行保持原来的状态,下一行接着用
        let Ok(ops) = self.state.parse_line(line, &SYNTAXES) else {
            return out;
        };
        let mut cursor = 0;
        for (index, op) in ops {
            self.push(cursor..index, &mut out);
            let _ = self.scopes.apply(&op);
            cursor = index;
        }
        self.push(cursor..line.len(), &mut out);
        out
    }

    fn push(&mut self, range: Range<usize>, out: &mut Vec<SyntaxSpan>) {
        if range.is_empty() {
            return;
        }
        // 最内层有名字的 scope 决定颜色,字符串的引号跟着字符串走
        let names = &mut self.names;
        let name = self
            .scopes
            .scopes
            .iter()
            .rev()
            .find_map(|s| *names.entry(*s).or_insert_with(|| semantic_name(*s)));
        let Some(name) = name else {
            return;
        };
        match out.last_mut() {
            Some((last, n)) if last.end == range.start && *n == name => last.end = range.end,
            _ => out.push((range, name)),
        }
    }
}

/// TextMate scope 到高亮名;`None` 表示交给外层 scope 决定。
/// 名字与 gpui-component 主题的高亮名一致(keyword、string、function…)。
fn semantic_name(scope: Scope) -> Option<&'static str> {
    let scope = scope.build_string();
    let is = |prefix: &str| {
        scope == prefix
            || scope
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('.'))
    };
    let name = if is("punctuation.definition.string")
        || is("punctuation.definition.comment")
        || is("invalid")
        || is("meta")
        || is("source")
        || is("text")
    {
        // 定界符属于它包住的内容;其余是没有自己颜色的容器
        return None;
    } else if is("comment") {
        "comment"
    } else if is("constant.character.escape") {
        "string.escape"
    } else if is("string.regexp") {
        "string.regex"
    } else if is("string") {
        "string"
    } else if is("constant.numeric") {
        "number"
    } else if is("constant.language") {
        "boolean"
    } else if is("constant") || is("support.constant") || is("entity.name.constant") {
        "constant"
    } else if is("keyword.operator") {
        // `new`、`typeof`、`instanceof` 这类单词运算符按关键字着色
        if scope.contains("new")
            || scope.contains("expression")
            || scope.contains("word")
            || scope.contains("logical.python")
        {
            "keyword"
        } else {
            "operator"
        }
    } else if is("keyword") || is("storage") {
        "keyword"
    } else if is("entity.name.function")
        || is("support.function")
        || is("variable.function")
        || is("meta.function-call")
    {
        "function"
    } else if is("entity.name.type")
        || is("entity.name.class")
        || is("entity.name.struct")
        || is("entity.name.enum")
        || is("entity.name.trait")
        || is("entity.name.interface")
        || is("entity.other.inherited-class")
        || is("support.type")
        || is("support.class")
    {
        "type"
    } else if is("entity.name.tag") {
        "tag"
    } else if is("entity.other.attribute-name") {
        "attribute"
    } else if is("entity.name.section") || is("markup.heading") {
        "title"
    } else if is("variable.language") {
        "variable.special"
    } else if is("markup.underline.link") {
        "link_uri"
    } else if is("markup.raw") {
        "text.literal"
    } else {
        return None;
    };
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{DiffLine, Hunk};

    fn line(kind: LineKind, old_no: Option<u32>, new_no: Option<u32>, text: &str) -> DiffLine {
        DiffLine {
            kind,
            old_no,
            new_no,
            text: text.into(),
            no_newline: false,
        }
    }

    fn names<'a>(text: &'a str, spans: &[SyntaxSpan]) -> Vec<(&'a str, &'static str)> {
        spans.iter().map(|(r, n)| (&text[r.clone()], *n)).collect()
    }

    fn file(path: &str, lines: Vec<DiffLine>) -> FileDiff {
        FileDiff {
            path: path.into(),
            hunks: vec![Hunk {
                header: String::new(),
                old_start: 1,
                old_lines: 0,
                new_start: 1,
                new_lines: 0,
                lines,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn unknown_language_is_none() {
        let f = file(
            "notes.unknownext",
            vec![line(LineKind::Added, None, Some(1), "x")],
        );
        assert!(highlight_diff(&f, None, Some("x\n")).is_none());
    }

    #[test]
    fn highlights_keywords_and_strings_from_full_file() {
        let new = "/* 开头的\n多行注释 */\nconst a = 'x';\n";
        let f = file(
            "a.js",
            vec![line(LineKind::Added, None, Some(2), "多行注释 */")],
        );
        let s = highlight_diff(&f, None, Some(new)).unwrap();
        // 从完整文件解析:注释的第二行仍被认作注释
        assert_eq!(
            names("多行注释 */", s.line(0, 0)),
            vec![("多行注释 */", "comment")]
        );

        let f = file(
            "a.js",
            vec![line(LineKind::Added, None, Some(3), "const a = 'x';")],
        );
        let s = highlight_diff(&f, None, Some(new)).unwrap();
        let got = names("const a = 'x';", s.line(0, 0));
        assert!(got.contains(&("const", "keyword")), "{got:?}");
        assert!(got.contains(&("'x'", "string")), "{got:?}");
    }

    #[test]
    fn deleted_lines_use_old_file_and_vue_script_is_javascript() {
        let old = "<template>\n  <div class=\"a\"></div>\n</template>\n<script>\nexport default {}\n</script>\n";
        let new = "<template>\n  <div class=\"b\"></div>\n</template>\n<script>\nexport default { name: 'x' }\n</script>\n";
        let f = file(
            "src/App.vue",
            vec![
                line(LineKind::Deleted, Some(5), None, "export default {}"),
                line(
                    LineKind::Added,
                    None,
                    Some(5),
                    "export default { name: 'x' }",
                ),
            ],
        );
        let s = highlight_diff(&f, Some(old), Some(new)).unwrap();
        assert!(names("export default {}", s.line(0, 0)).contains(&("export", "keyword")));
        let got = names("export default { name: 'x' }", s.line(0, 1));
        assert!(got.contains(&("'x'", "string")), "{got:?}");
    }

    #[test]
    fn falls_back_to_hunk_when_file_missing_or_mismatched() {
        let f = file(
            "a.rs",
            vec![
                line(LineKind::Context, Some(1), Some(1), "fn main() {"),
                line(LineKind::Added, None, Some(2), "    let s = \"hi\";"),
            ],
        );
        // 文件内容与 diff 对不上(比如刚被改过):按 hunk 解析
        let s = highlight_diff(&f, None, Some("完全不同\n的内容\n")).unwrap();
        assert!(names("fn main() {", s.line(0, 0)).contains(&("fn", "keyword")));
        let got = names("    let s = \"hi\";", s.line(0, 1));
        assert!(got.contains(&("\"hi\"", "string")), "{got:?}");
    }

    #[test]
    fn crlf_and_spans_stay_within_line() {
        let new = "let a = 1;\r\n";
        let f = file(
            "a.ts",
            vec![line(LineKind::Added, None, Some(1), "let a = 1;")],
        );
        let s = highlight_diff(&f, None, Some(new)).unwrap();
        assert!(
            s.line(0, 0)
                .iter()
                .all(|(r, _)| r.end <= "let a = 1;".len())
        );
        assert!(names("let a = 1;", s.line(0, 0)).contains(&("1", "number")));
    }

    #[test]
    fn highlight_source_offsets_are_relative_to_the_whole_text() {
        let text = "// c\nfn main() {}";
        let spans = highlight_source("a.rs", text);
        let fn_at = text.find("fn").unwrap();
        assert!(
            spans
                .iter()
                .any(|(r, n)| *n == "keyword" && r.start == fn_at)
        );
        assert!(spans.iter().all(|(r, _)| r.end <= text.len()));
        // 不认识的语言没有高亮
        assert!(highlight_source("a.unknown-ext", text).is_empty());
    }
}
