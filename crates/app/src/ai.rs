//! AI 生成提交信息:配置、提示词、请求与响应的构造和解析,以及经系统 `curl` 发出的调用。
//!
//! 与更新、头像一样不引入 HTTP 依赖。请求体和请求头(含 API Key)都经临时文件 / stdin
//! 交给 curl,不出现在命令行参数里(`ps` 看不到)。除构造请求外都是纯函数,可脱离网络测试。
//!
//! @author ixxxxoooo

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Write as _;
use std::process::{Command, Stdio};

/// 接口协议。OpenAI 兼容覆盖 OpenAI、DeepSeek、通义、Ollama、各类中转等。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiProvider {
    #[default]
    OpenAi,
    Anthropic,
}

impl AiProvider {
    pub const ALL: [AiProvider; 2] = [AiProvider::OpenAi, AiProvider::Anthropic];

    pub fn label(self) -> &'static str {
        match self {
            AiProvider::OpenAi => "OpenAI 兼容",
            AiProvider::Anthropic => "Anthropic",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            AiProvider::OpenAi => "https://api.openai.com/v1",
            AiProvider::Anthropic => "https://api.anthropic.com",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            AiProvider::OpenAi => "gpt-4o-mini",
            AiProvider::Anthropic => "claude-haiku-4-5",
        }
    }
}

/// 提交信息的格式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommitStyle {
    /// `type(scope): 摘要`,必要时空行后列要点。
    #[default]
    Conventional,
    /// 只有一行摘要。
    Simple,
    /// 摘要加要点列表。
    Detailed,
}

impl CommitStyle {
    pub const ALL: [CommitStyle; 3] = [
        CommitStyle::Conventional,
        CommitStyle::Simple,
        CommitStyle::Detailed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CommitStyle::Conventional => "Conventional Commits",
            CommitStyle::Simple => "一行摘要",
            CommitStyle::Detailed => "摘要 + 要点",
        }
    }
}

/// 提交信息使用的语言。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommitLanguage {
    #[default]
    Chinese,
    English,
}

impl CommitLanguage {
    pub const ALL: [CommitLanguage; 2] = [CommitLanguage::Chinese, CommitLanguage::English];

    pub fn label(self) -> &'static str {
        match self {
            CommitLanguage::Chinese => "中文",
            CommitLanguage::English => "English",
        }
    }
}

/// AI 配置,随应用设置保存(API Key 目前是明文存在设置文件里)。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    pub provider: AiProvider,
    /// 接口根地址;留空用协议的默认值。
    pub base_url: String,
    /// 模型名;留空用协议的默认值。
    pub model: String,
    pub api_key: String,
    pub style: CommitStyle,
    pub language: CommitLanguage,
    /// 附加要求,原样追加在提示词末尾。
    pub instructions: String,
}

impl AiConfig {
    pub fn effective_base_url(&self) -> String {
        let url = self.base_url.trim().trim_end_matches('/');
        if url.is_empty() {
            self.provider.default_base_url().to_string()
        } else {
            url.to_string()
        }
    }

    pub fn effective_model(&self) -> String {
        let model = self.model.trim();
        if model.is_empty() {
            self.provider.default_model().to_string()
        } else {
            model.to_string()
        }
    }

    /// 本机服务(如 Ollama)通常不需要 Key。
    fn is_local(&self) -> bool {
        let url = self.effective_base_url();
        ["://localhost", "://127.0.0.1", "://[::1]"]
            .iter()
            .any(|h| url.contains(h))
    }

    /// 配置能否发起请求;不能时返回要提示用户的原因。
    pub fn check(&self) -> Result<(), String> {
        let url = self.effective_base_url();
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err("接口地址要以 http:// 或 https:// 开头".into());
        }
        let bad = |s: &str| s.chars().any(char::is_control);
        if bad(&url) || bad(&self.api_key) || bad(&self.effective_model()) {
            return Err("地址、模型或 Key 里含有换行等控制字符".into());
        }
        if self.api_key.trim().is_empty() && !self.is_local() {
            return Err("还没有填 API Key".into());
        }
        Ok(())
    }
}

/// 提示词:(系统提示, 用户消息)。`recent` 是仓库最近的提交标题。
pub fn build_prompt(
    config: &AiConfig,
    patch: &str,
    truncated: bool,
    recent: &[String],
) -> (String, String) {
    let language = match config.language {
        CommitLanguage::Chinese => "中文",
        CommitLanguage::English => "English",
    };
    let style = match config.style {
        CommitStyle::Conventional => {
            "格式:`type(scope): 摘要`,type 取 feat / fix / refactor / perf / docs / test / build / ci / chore / style 之一,scope 可省略;首行不超过 72 个字符;改动较多时空一行后用 `- ` 列出要点。"
        }
        CommitStyle::Simple => "只写一行摘要,不超过 72 个字符,不要正文。",
        CommitStyle::Detailed => "首行一句话摘要(不超过 72 个字符),空一行后用 `- ` 列出要点。",
    };
    let mut system = format!(
        "你是资深工程师,根据 git diff 为这次提交写提交信息。{style}\n用{language}写;说明“做了什么、为什么”,不要逐行复述 diff,不要编造 diff 里没有的内容。\n只输出提交信息本身:不要解释,不要代码块围栏,不要引号。"
    );
    let extra = config.instructions.trim();
    if !extra.is_empty() {
        system.push_str("\n附加要求:");
        system.push_str(extra);
    }
    let mut user = String::new();
    if !recent.is_empty() {
        user.push_str("仓库最近的提交标题(可参考写法):\n");
        for subject in recent {
            user.push_str(&format!("- {subject}\n"));
        }
        user.push('\n');
    }
    user.push_str("这次提交的 diff:\n");
    user.push_str(patch);
    if truncated {
        user.push_str("\n(diff 过长,后面的部分已省略)\n");
    }
    (system, user)
}

/// 请求:(地址, 请求头, 请求体)。
pub fn build_request(
    config: &AiConfig,
    system: &str,
    user: &str,
) -> (String, Vec<(String, String)>, String) {
    let base = config.effective_base_url();
    let model = config.effective_model();
    let key = config.api_key.trim();
    let json_header = ("Content-Type".to_string(), "application/json".to_string());
    match config.provider {
        AiProvider::OpenAi => {
            let mut headers = vec![json_header];
            if !key.is_empty() {
                headers.push(("Authorization".into(), format!("Bearer {key}")));
            }
            let body = json!({
                "model": model,
                "temperature": 0.2,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
            });
            (
                format!("{base}/chat/completions"),
                headers,
                body.to_string(),
            )
        }
        AiProvider::Anthropic => {
            let headers = vec![
                json_header,
                ("x-api-key".into(), key.to_string()),
                ("anthropic-version".into(), "2023-06-01".into()),
            ];
            let body = json!({
                "model": model,
                "max_tokens": 1024,
                "temperature": 0.2,
                "system": system,
                "messages": [{"role": "user", "content": user}],
            });
            (format!("{base}/v1/messages"), headers, body.to_string())
        }
    }
}

/// 解析响应,取出提交信息;接口返回错误或格式不对时给出可读的原因。
pub fn parse_response(provider: AiProvider, body: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| {
        let head: String = body.chars().take(120).collect();
        format!("响应不是 JSON:{head}")
    })?;
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| err.as_str())
            .unwrap_or("接口返回了错误");
        return Err(msg.to_string());
    }
    let text = match provider {
        AiProvider::OpenAi => v["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string),
        AiProvider::Anthropic => v["content"].as_array().map(|parts| {
            parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        }),
    };
    let message = text.map(|t| clean_message(&t)).unwrap_or_default();
    if message.is_empty() {
        Err("模型没有返回内容".into())
    } else {
        Ok(message)
    }
}

/// 去掉模型常加的代码块围栏和首尾引号。
pub fn clean_message(text: &str) -> String {
    let mut t = text.trim();
    if let Some(rest) = t.strip_prefix("```") {
        t = rest.split_once('\n').map_or("", |(_, body)| body);
        t = t.trim_end().strip_suffix("```").unwrap_or(t);
    }
    let t = t.trim();
    let unquoted = [('"', '"'), ('“', '”'), ('`', '`')]
        .iter()
        .find_map(|&(a, b)| t.strip_prefix(a)?.strip_suffix(b))
        .unwrap_or(t);
    unquoted.trim().to_string()
}

/// curl 配置文件里的字符串值要转义反斜杠与双引号。
fn curl_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// 发请求并解析。阻塞,放到后台线程里调用。
pub fn generate(
    config: &AiConfig,
    patch: &str,
    truncated: bool,
    recent: &[String],
) -> Result<String, String> {
    config.check()?;
    let (system, user) = build_prompt(config, patch, truncated, recent);
    let (url, headers, body) = build_request(config, &system, &user);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let file = std::env::temp_dir().join(format!("gitlane-ai-{}-{nanos}.json", std::process::id()));
    std::fs::write(&file, body).map_err(|e| format!("写临时文件失败:{e}"))?;
    let mut conf = format!(
        "url = {}\nrequest = \"POST\"\nsilent\nshow-error\nmax-time = 90\ndata-binary = {}\n",
        curl_quote(&url),
        curl_quote(&format!("@{}", file.display())),
    );
    for (k, v) in &headers {
        conf.push_str(&format!("header = {}\n", curl_quote(&format!("{k}: {v}"))));
    }
    let result = run_curl(&conf);
    let _ = std::fs::remove_file(&file);
    parse_response(config.provider, &result?)
}

fn run_curl(conf: &str) -> Result<String, String> {
    let mut child = Command::new("/usr/bin/curl")
        .args(["-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法运行 curl:{e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(conf.as_bytes())
            .map_err(|e| format!("无法运行 curl:{e}"))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("无法运行 curl:{e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("请求失败:{}", err.trim()))
    }
}

#[cfg(test)]
#[path = "ai_tests.rs"]
mod tests;
