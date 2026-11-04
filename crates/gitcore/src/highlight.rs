//! diff 的语法高亮:用 syntect(bat 的语法集,含 Vue / TypeScript)给每行标出高亮名。
//!
//! 只产出 “keyword”、“string” 这类与主题无关的名字,颜色由界面层按亮暗主题决定。
//! 尽量用完整的旧 / 新文件从头解析,这样跨行的注释、模板字符串、Vue 里的 `<script>` 都能着色正确;
//! 拿不到完整文件(或太大)时退回只解析 hunk 里的行。

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

    fn names<'a>(text: &'a str, spans: &[SyntaxSpan]) -> Vec<(&'a str, &'static str)> {
        spans.iter().map(|(r, n)| (&text[r.clone()], *n)).collect()
    }

    #[test]
    fn unknown_language_has_no_highlight() {
        assert!(highlight_source("notes.unknownext", "x\n").is_empty());
    }

    #[test]
    fn offsets_are_relative_to_the_whole_text() {
        let text = "// c\nfn main() {}";
        let spans = highlight_source("a.rs", text);
        let fn_at = text.find("fn").unwrap();
        assert!(
            spans
                .iter()
                .any(|(r, n)| *n == "keyword" && r.start == fn_at)
        );
        assert!(spans.iter().all(|(r, _)| r.end <= text.len()));
    }

    #[test]
    fn multiline_comment_and_strings_are_highlighted() {
        let text = "/* 开头的\n多行注释 */\nconst a = 'x';";
        let got = names(text, &highlight_source("a.js", text));
        assert!(got.contains(&("多行注释 */", "comment")), "{got:?}");
        assert!(got.contains(&("const", "keyword")), "{got:?}");
        assert!(got.contains(&("'x'", "string")), "{got:?}");
    }

    #[test]
    fn vue_script_is_javascript() {
        let text = "<template>\n  <div class=\"a\"></div>\n</template>\n<script>\nexport default { name: 'x' }\n</script>";
        let got = names(text, &highlight_source("src/App.vue", text));
        assert!(got.contains(&("export", "keyword")), "{got:?}");
        assert!(got.contains(&("'x'", "string")), "{got:?}");
    }

    #[test]
    fn crlf_spans_stay_within_text() {
        let text = "let a = 1;\r\nlet b = 2;";
        let spans = highlight_source("a.ts", text);
        assert!(spans.iter().all(|(r, _)| r.end <= text.len()));
        assert!(names(text, &spans).contains(&("1", "number")));
    }
}
