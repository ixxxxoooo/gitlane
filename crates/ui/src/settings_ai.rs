//! 设置窗口的 “AI” 页:配置用来生成提交信息的模型接口(协议、地址、模型、Key、格式与语言)。
//!
//! 文本项在失焦或回车时才写入设置;下拉项选中即写入。切换协议会清空地址与模型,
//! 因为两种协议的默认值不同、各自填的内容不能互用。
//!
//! @author ixxxxoooo

use crate::settings_view::{SettingsView, choice};
use app::{AiConfig, AiProvider, CommitLanguage, CommitStyle, Settings as AppSettings, Workspace};
use gpui::component::Icon;
use gpui::component::input::{Input, InputEvent, InputState};
use gpui::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui::prelude::*;
use gpui::*;

#[derive(Clone, Copy)]
pub(crate) enum AiKey {
    BaseUrl,
    Model,
    ApiKey,
    Instructions,
}

impl AiKey {
    pub(crate) const ALL: [AiKey; 4] = [
        AiKey::BaseUrl,
        AiKey::Model,
        AiKey::ApiKey,
        AiKey::Instructions,
    ];

    fn get(self, c: &AiConfig) -> &str {
        match self {
            AiKey::BaseUrl => &c.base_url,
            AiKey::Model => &c.model,
            AiKey::ApiKey => &c.api_key,
            AiKey::Instructions => &c.instructions,
        }
    }

    fn set(self, c: &mut AiConfig, value: String) {
        match self {
            AiKey::BaseUrl => c.base_url = value,
            AiKey::Model => c.model = value,
            AiKey::ApiKey => c.api_key = value,
            AiKey::Instructions => c.instructions = value,
        }
    }
}

/// 与 `AiKey::ALL` 一一对应的四个输入框。
pub(crate) struct AiInputs(pub [Entity<InputState>; 4]);

impl AiInputs {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<SettingsView>) -> Self {
        let placeholders = [
            "留空用协议的默认地址",
            "留空用协议的默认模型",
            "sk-…",
            "例如:标题不要加句号;涉及接口变更时写在正文里",
        ];
        let inputs = placeholders.map(|p| cx.new(|cx| InputState::new(window, cx).placeholder(p)));
        inputs[AiKey::ApiKey as usize].update(cx, |s, cx| s.set_masked(true, window, cx));
        Self(inputs)
    }

    pub(crate) fn input(&self, key: AiKey) -> &Entity<InputState> {
        &self.0[key as usize]
    }

    /// 失焦或回车时把输入框的值写进设置。
    pub(crate) fn subscribe(
        &self,
        window: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Vec<Subscription> {
        AiKey::ALL
            .iter()
            .map(|&key| {
                cx.subscribe_in(
                    self.input(key),
                    window,
                    move |this, _, e: &InputEvent, _, cx| {
                        if matches!(e, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                            let value = this
                                .ai_inputs
                                .input(key)
                                .read(cx)
                                .value()
                                .trim()
                                .to_string();
                            this.workspace.update(cx, |ws, cx| {
                                if key.get(&ws.settings.ai) != value {
                                    ws.update_settings(cx, |s| key.set(&mut s.ai, value));
                                }
                            });
                        }
                    },
                )
            })
            .collect()
    }

    /// 设置变化(含切换协议清空地址与模型)后,把值同步到没在编辑的输入框。
    pub(crate) fn sync(&self, ws: &AiConfig, window: &mut Window, cx: &mut App) {
        for key in AiKey::ALL {
            let input = self.input(key).clone();
            let want = key.get(ws).to_string();
            if !input.focus_handle(cx).is_focused(window) && input.read(cx).value() != want {
                input.update(cx, |s, cx| s.set_value(want, window, cx));
            }
        }
    }
}

/// “AI”设置页。
pub(crate) fn ai_page(ws: &Entity<Workspace>, inputs: &AiInputs) -> SettingPage {
    let text_item = |title: &'static str, key: AiKey, width: f32| {
        let input = inputs.input(key).clone();
        SettingItem::new(
            title,
            SettingField::render(move |_, _, _| Input::new(&input).w(px(width))),
        )
    };
    SettingPage::new("AI")
        .icon(Icon::from(gpui::assets::IconName::Sparkles))
        .description(
            "配置后,提交框右下角的 ✦ 会根据待提交的改动生成提交信息。diff 会发送给你填写的接口,请确认可以外发。",
        )
        .resettable(false)
        .groups([
            SettingGroup::new().title("模型接口").items([
                SettingItem::new(
                    "协议",
                    choice(
                        ws,
                        AiProvider::ALL.map(|p| (p, p.label())).to_vec(),
                        AppSettings::default().ai.provider,
                        |s| s.ai.provider,
                        |s, v| {
                            if s.ai.provider != v {
                                s.ai.provider = v;
                                s.ai.base_url.clear();
                                s.ai.model.clear();
                            }
                        },
                    ),
                )
                .description(
                    "OpenAI 兼容(/chat/completions)可用于 OpenAI、DeepSeek、通义、Ollama 和各类中转;Anthropic 用 /v1/messages。",
                ),
                text_item("接口地址", AiKey::BaseUrl, 300.).description(format!(
                    "默认:OpenAI 兼容 {};Anthropic {}。本机服务如 http://localhost:11434/v1 不需要 Key。",
                    AiProvider::OpenAi.default_base_url(),
                    AiProvider::Anthropic.default_base_url()
                )),
                text_item("模型", AiKey::Model, 300.).description(format!(
                    "默认:OpenAI 兼容 {};Anthropic {}。",
                    AiProvider::OpenAi.default_model(),
                    AiProvider::Anthropic.default_model()
                )),
                text_item("API Key", AiKey::ApiKey, 300.).description(
                    "保存在本机的设置文件里(明文),只发给上面的接口地址;不会写进日志。",
                ),
            ]),
            SettingGroup::new().title("提交信息").items([
                SettingItem::new(
                    "格式",
                    choice(
                        ws,
                        CommitStyle::ALL.map(|c| (c, c.label())).to_vec(),
                        AppSettings::default().ai.style,
                        |s| s.ai.style,
                        |s, v| s.ai.style = v,
                    ),
                ),
                SettingItem::new(
                    "语言",
                    choice(
                        ws,
                        CommitLanguage::ALL.map(|c| (c, c.label())).to_vec(),
                        AppSettings::default().ai.language,
                        |s| s.ai.language,
                        |s, v| s.ai.language = v,
                    ),
                ),
                text_item("附加要求", AiKey::Instructions, 300.)
                    .description("追加到提示词末尾,用来约束风格、术语等。"),
            ]),
        ])
}
