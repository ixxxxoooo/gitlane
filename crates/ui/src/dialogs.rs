//! 通用对话框:危险操作确认、单行文本输入。

use gpui::component::button::ButtonVariant;
use gpui::component::input::{Input, InputEvent, InputState};
use gpui::component::{WindowExt, v_flex};
use gpui::prelude::*;
use gpui::*;
use std::cell::Cell;
use std::rc::Rc;

/// 危险操作确认。`lines` 列出将受影响的提交或文件。
pub fn confirm(
    window: &mut Window,
    cx: &mut App,
    title: impl Into<SharedString>,
    lines: Vec<String>,
    ok_label: impl Into<SharedString>,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
) {
    let title = title.into();
    let ok_label = ok_label.into();
    let on_ok = Rc::new(on_ok);
    window.open_dialog(cx, move |dialog, _, _| {
        let on_ok = on_ok.clone();
        dialog
            .title(title.clone())
            .w(px(520.))
            .child(
                v_flex()
                    .gap_1()
                    .text_sm()
                    .max_h(px(360.))
                    .overflow_hidden()
                    .children(lines.iter().map(|l| div().child(l.clone()))),
            )
            .button_props(
                gpui::component::dialog::DialogButtonProps::default()
                    .ok_text(ok_label.clone())
                    .ok_variant(ButtonVariant::Danger)
                    .cancel_text("取消")
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| {
                on_ok(window, cx);
                true
            })
    });
}

type OnText = dyn Fn(String, &mut Window, &mut App);

/// 单行文本输入。回车或点确定提交,内容为空时不关闭。
pub fn prompt_text(
    window: &mut Window,
    cx: &mut App,
    title: impl Into<SharedString>,
    placeholder: &'static str,
    initial: String,
    on_ok: impl Fn(String, &mut Window, &mut App) + 'static,
) {
    let title = title.into();
    let input = cx.new(|cx| {
        let mut s = InputState::new(window, cx).placeholder(placeholder);
        s.set_value(initial, window, cx);
        s
    });
    let on_ok: Rc<OnText> = Rc::new(on_ok);

    let submit = {
        let input = input.clone();
        let on_ok = on_ok.clone();
        move |window: &mut Window, cx: &mut App| -> bool {
            let value = input.read(cx).value().trim().to_string();
            if value.is_empty() {
                return false;
            }
            on_ok(value, window, cx);
            true
        }
    };
    let submit = Rc::new(submit);

    // 输入框内回车直接提交
    let sub = {
        let submit = submit.clone();
        window.subscribe(&input, cx, move |_, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { .. } = event
                && submit(window, cx)
            {
                window.close_dialog(cx);
            }
        })
    };
    let sub = Rc::new(Cell::new(Some(sub)));

    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            input.focus_handle(cx).focus(window, cx);
        }
        let submit = submit.clone();
        let sub = sub.clone();
        dialog
            .title(title.clone())
            .w(px(440.))
            .child(Input::new(&input))
            .button_props(
                gpui::component::dialog::DialogButtonProps::default()
                    .ok_text("确定")
                    .cancel_text("取消")
                    .show_cancel(true),
            )
            .on_ok(move |_, window, cx| submit(window, cx))
            .on_close(move |_, _, _| {
                sub.take();
            })
    });
}
