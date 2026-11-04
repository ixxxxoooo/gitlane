//! 命令面板(⌘K):模糊搜索全部动作。

use crate::actions::palette_entries;
use crate::format::fuzzy_match;
use gpui::component::input::{Input, InputEvent, InputState};
use gpui::component::{ActiveTheme, WindowExt, h_flex, v_flex};
use gpui::prelude::*;
use gpui::*;
use std::rc::Rc;

pub type OnPick = Rc<dyn Fn(Box<dyn Action>, &mut Window, &mut App)>;

pub struct PaletteView {
    input: Entity<InputState>,
    on_pick: OnPick,
    _sub: Subscription,
}

impl PaletteView {
    pub fn new(on_pick: OnPick, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("输入命令…"));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => {
                    if let Some(i) = this.matches(cx).first().copied() {
                        this.pick(i, window, cx);
                    }
                }
                _ => {}
            },
        );
        Self {
            input,
            on_pick,
            _sub: sub,
        }
    }

    pub fn input_focus(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }

    fn matches(&self, cx: &App) -> Vec<usize> {
        let q = self.input.read(cx).value().to_string();
        palette_entries()
            .iter()
            .enumerate()
            .filter(|(_, (name, _, _))| fuzzy_match(name, &q))
            .map(|(i, _)| i)
            .collect()
    }

    fn pick(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, _, action)) = palette_entries().into_iter().nth(index) else {
            return;
        };
        window.close_dialog(cx);
        (self.on_pick)(action, window, cx);
    }
}

impl Render for PaletteView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let entries = palette_entries();
        let matches = self.matches(cx);
        v_flex().gap_2().child(Input::new(&self.input)).child(
            div()
                .id("palette-list")
                .max_h(px(360.))
                .overflow_y_scroll()
                .children(matches.into_iter().enumerate().map(|(rank, i)| {
                    let (name, key, _) = &entries[i];
                    h_flex()
                        .id(("palette", i))
                        .px_2()
                        .h(px(28.))
                        .rounded_sm()
                        .text_sm()
                        .cursor_pointer()
                        .when(rank == 0, |d| d.bg(theme.list_active))
                        .hover(|s| s.bg(theme.list_hover))
                        .child(div().flex_1().child(*name))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(*key),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| this.pick(i, window, cx)))
                })),
        )
    }
}
