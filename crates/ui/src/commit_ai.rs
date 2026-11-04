//! 提交框右下角的 ✦:按待提交的改动调用配置好的模型生成提交信息,填进输入框。
//!
//! 只读仓库(git2)、联网在后台线程;生成期间按钮转圈,再点无效。结果覆盖输入框当前内容,
//! 用户可以直接改。没配置好时提示原因并打开设置。
//!
//! @author ixxxxoooo

use crate::actions::OpenSettings;
use crate::repo_view::RepoView;
use app::ai;
use gitcore::Repo;
use gitcore::change_context::{change_context, recent_subjects};
use gpui::component::WindowExt as _;
use gpui::component::notification::Notification;
use gpui::*;

/// 发给模型的 diff 最多多少字节。
const PATCH_LIMIT: usize = 24_000;
/// 带上几条最近的提交标题,让生成的信息沿用仓库的写法。
const RECENT_SUBJECTS: usize = 5;

/// 后台任务:读改动、调模型。返回提交信息或可读的失败原因。
fn generate_for(path: std::path::PathBuf, config: ai::AiConfig) -> Result<String, String> {
    let repo = Repo::open(&path).map_err(|e| format!("打开仓库失败:{e}"))?;
    let ctx = change_context(&repo, PATCH_LIMIT).map_err(|e| format!("读取改动失败:{e}"))?;
    if ctx.files == 0 {
        return Err("没有可提交的改动".into());
    }
    let recent = recent_subjects(&repo, RECENT_SUBJECTS);
    ai::generate(&config, &ctx.patch, ctx.truncated, &recent)
}

impl RepoView {
    /// 点击 ✦:生成提交信息。
    pub(crate) fn generate_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_busy {
            return;
        }
        let Some(config) = self
            .workspace
            .upgrade()
            .map(|ws| ws.read(cx).settings.ai.clone())
        else {
            return;
        };
        if let Err(reason) = config.check() {
            window.push_notification(
                Notification::warning(format!("{reason},请在 设置 → AI 里配置")),
                cx,
            );
            window.dispatch_action(Box::new(OpenSettings), cx);
            return;
        }
        let path = self.repo.read(cx).path.clone();
        self.ai_busy = true;
        cx.notify();
        let task = cx.background_spawn(async move { generate_for(path, config) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.ai_busy = false;
                match result {
                    Ok(message) => this
                        .message
                        .update(cx, |m, cx| m.set_value(message, window, cx)),
                    Err(e) => window.push_notification(
                        Notification::error(format!("生成提交信息失败:{e}")),
                        cx,
                    ),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
