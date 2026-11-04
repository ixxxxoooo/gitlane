//! 右栏的面板(提交详情 / 未暂存 / 已暂存):展开的面板平分剩余高度,收起的只剩标题行,
//! 顺序固定。和左侧引用树的面板行为一致。

use crate::repo_view::RepoView;
use gpui::base::TestSupportExt as _;
use gpui::component::{ActiveTheme, Icon, IconName, Sizable, h_flex, v_flex};
use gpui::prelude::*;
use gpui::*;

/// 右栏面板标题行高度。
const HEADER_HEIGHT: f32 = 32.;

impl RepoView {
    /// 一个右栏面板:标题行(点击折叠 / 展开,可带一个操作按钮)+ 展开时占 `flex_1` 的内容区。
    ///
    /// 参数:`key` 面板标识,同时是折叠状态的键;`head` 标题(可选图标, 文字);`button` 标题行右侧的按钮;
    /// `body` 内容(自己负责滚动);`collapsible` 为假时面板固定展开、没有折叠箭头(单面板场景)。
    pub(crate) fn side_section(
        &self,
        key: &'static str,
        collapsible: bool,
        head: (Option<Icon>, String),
        button: Option<impl IntoElement>,
        body: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (icon, title) = head;
        let theme = cx.theme().clone();
        let open = !collapsible || !self.side_collapsed.contains(key);
        // 点击只由标题左半部分处理,右侧按钮是独立元素,两边不会同时触发
        let toggle = h_flex()
            .id(SharedString::from(format!("side-toggle-{key}")))
            .flex_1()
            .h_full()
            .gap_1p5()
            .when(collapsible, |d| {
                d.cursor_pointer()
                    .child(
                        Icon::new(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall(),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.side_collapsed.remove(key) {
                            this.side_collapsed.insert(key);
                        }
                        this.save_layout(cx);
                        cx.notify();
                    }))
            })
            .children(icon.map(|i| i.xsmall()))
            .child(title)
            .test_support();
        v_flex()
            .id(SharedString::from(format!("side-section-{key}")))
            .test_support()
            .when(open, |d| d.flex_1())
            .min_h_0()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .h(px(HEADER_HEIGHT))
                    .flex_none()
                    .pl_2()
                    .pr_3()
                    .gap_2()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(toggle)
                    .children(button),
            )
            .when(open, |d| {
                d.child(div().flex_1().min_h_0().overflow_hidden().child(body))
            })
            .into_any_element()
    }
}
