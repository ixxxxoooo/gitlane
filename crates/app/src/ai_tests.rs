use super::{
    AiConfig, AiProvider, CommitLanguage, CommitStyle, build_prompt, build_request, clean_message,
    curl_quote, parse_response,
};

fn config(provider: AiProvider) -> AiConfig {
    AiConfig {
        provider,
        api_key: "sk-test".into(),
        ..Default::default()
    }
}

#[test]
fn blank_url_and_model_fall_back_to_provider_defaults() {
    let c = config(AiProvider::Anthropic);
    assert_eq!(c.effective_base_url(), "https://api.anthropic.com");
    assert_eq!(c.effective_model(), "claude-haiku-4-5");
    let c = AiConfig {
        base_url: " http://host/v1/ ".into(),
        model: "m".into(),
        ..config(AiProvider::OpenAi)
    };
    assert_eq!(c.effective_base_url(), "http://host/v1");
    assert_eq!(c.effective_model(), "m");
}

#[test]
fn check_requires_key_except_for_local_services() {
    assert!(config(AiProvider::OpenAi).check().is_ok());
    let no_key = AiConfig::default();
    assert_eq!(no_key.check(), Err("还没有填 API Key".into()));
    let local = AiConfig {
        base_url: "http://localhost:11434/v1".into(),
        ..Default::default()
    };
    assert!(local.check().is_ok());
    let bad = AiConfig {
        base_url: "ftp://x".into(),
        ..config(AiProvider::OpenAi)
    };
    assert!(bad.check().is_err());
    let newline = AiConfig {
        api_key: "a\nb".into(),
        ..config(AiProvider::OpenAi)
    };
    assert!(newline.check().is_err());
}

#[test]
fn openai_and_anthropic_requests_differ_in_shape() {
    let (url, headers, body) = build_request(&config(AiProvider::OpenAi), "sys", "usr");
    assert_eq!(url, "https://api.openai.com/v1/chat/completions");
    assert!(headers.contains(&("Authorization".into(), "Bearer sk-test".into())));
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["messages"][0]["role"], "system");
    assert_eq!(v["messages"][1]["content"], "usr");

    let (url, headers, body) = build_request(&config(AiProvider::Anthropic), "sys", "usr");
    assert_eq!(url, "https://api.anthropic.com/v1/messages");
    assert!(headers.contains(&("x-api-key".into(), "sk-test".into())));
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["system"], "sys");
    assert_eq!(v["messages"][0]["content"], "usr");
}

#[test]
fn prompt_follows_style_language_and_extra_instructions() {
    let c = AiConfig {
        style: CommitStyle::Simple,
        language: CommitLanguage::English,
        instructions: "不要用句号".into(),
        ..config(AiProvider::OpenAi)
    };
    let (system, user) = build_prompt(&c, "+x\n", true, &["feat: a".to_string()]);
    assert!(
        system.contains("English") && system.contains("只写一行") && system.contains("不要用句号")
    );
    assert!(user.contains("- feat: a") && user.contains("+x") && user.contains("已省略"));
    let (_, plain) = build_prompt(&c, "+x\n", false, &[]);
    assert!(!plain.contains("最近的提交标题") && !plain.contains("已省略"));
}

#[test]
fn responses_are_parsed_per_provider_and_errors_surface() {
    let openai = r#"{"choices":[{"message":{"content":"feat: 加入 AI"}}]}"#;
    assert_eq!(
        parse_response(AiProvider::OpenAi, openai).unwrap(),
        "feat: 加入 AI"
    );
    let claude = r#"{"content":[{"type":"text","text":"fix: a"},{"type":"text","text":" b"}]}"#;
    assert_eq!(
        parse_response(AiProvider::Anthropic, claude).unwrap(),
        "fix: a b"
    );
    let err = r#"{"error":{"message":"invalid api key"}}"#;
    assert_eq!(
        parse_response(AiProvider::OpenAi, err),
        Err("invalid api key".into())
    );
    assert!(parse_response(AiProvider::OpenAi, "<html>").is_err());
    assert!(parse_response(AiProvider::OpenAi, r#"{"choices":[]}"#).is_err());
}

#[test]
fn clean_message_strips_fences_and_quotes() {
    assert_eq!(clean_message("```\nfeat: a\n\n- b\n```"), "feat: a\n\n- b");
    assert_eq!(clean_message("```text\nfix: x\n```"), "fix: x");
    assert_eq!(clean_message("\"feat: a\""), "feat: a");
    assert_eq!(clean_message("“feat: a”\n"), "feat: a");
    assert_eq!(clean_message("feat: a"), "feat: a");
}

#[test]
fn curl_quote_escapes_backslash_and_quote() {
    assert_eq!(curl_quote(r#"a"b\c"#), r#""a\"b\\c""#);
}
