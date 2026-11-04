//! 扫描目录:在后台查找目录下的 Git 仓库,列表勾选后批量添加为标签。

use app::Workspace;
use gitcore::scan::{DEFAULT_MAX_DEPTH, find_repos};
use gpui::base::TestSupportExt as _;
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::checkbox::Checkbox;
use gpui::component::input::{Input, InputEvent, InputState};
use gpui::component::{ActiveTheme, Disableable as _, Sizable, WindowExt, h_flex, v_flex};
use gpui::prelude::*;
use gpui::*;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;

/// 点“添加”时回调,参数是勾选的仓库路径。
pub type OnAdd = Rc<dyn Fn(Vec<PathBuf>, &mut Window, &mut App)>;

const ROW_HEIGHT: f32 = 28.;
const LIST_MAX_HEIGHT: f32 = 380.;

pub struct ScanEntry {
    pub path: PathBuf,
    /// 相对扫描根目录的显示名。
    pub label: String,
    pub already_open: bool,
}

pub struct ScanView {
    pub root: PathBuf,
    /// `None` 表示扫描中。
    pub entries: Option<Vec<ScanEntry>>,
    pub selected: BTreeSet<usize>,
    filter: Entity<InputState>,
    workspace: WeakEntity<Workspace>,
    on_add: OnAdd,
    /// 当前筛选下可见的条目下标,供虚拟列表按行取用。
    visible_rows: Rc<Vec<usize>>,
    list_scroll: UniformListScrollHandle,
    _sub: Subscription,
}

impl ScanView {
    pub fn new(
        root: PathBuf,
        workspace: WeakEntity<Workspace>,
        on_add: OnAdd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("筛选路径"));
        let sub = cx.subscribe(&filter, |_, _, e: &InputEvent, cx| {
            if let InputEvent::Change = e {
                cx.notify();
            }
        });

        let scan_root = root.clone();
        let task = cx.background_spawn(async move { find_repos(&scan_root, DEFAULT_MAX_DEPTH) });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                let ws = this.workspace.upgrade();
                let entries: Vec<ScanEntry> = found
                    .into_iter()
                    .map(|path| {
                        let already_open =
                            ws.as_ref().is_some_and(|w| w.read(cx).is_open(&path, cx));
                        let label = match path.strip_prefix(&this.root) {
                            Ok(rel) if !rel.as_os_str().is_empty() => rel.display().to_string(),
                            _ => path.file_name().map_or_else(
                                || path.display().to_string(),
                                |n| n.to_string_lossy().into_owned(),
                            ),
                        };
                        ScanEntry {
                            path,
                            label,
                            already_open,
                        }
                    })
                    .collect();
                // 默认勾选全部尚未打开的仓库
                this.selected = (0..entries.len())
                    .filter(|&i| !entries[i].already_open)
                    .collect();
                this.entries = Some(entries);
                cx.notify();
            });
        })
        .detach();

        Self {
            root,
            entries: None,
            selected: BTreeSet::new(),
            filter,
            workspace,
            on_add,
            visible_rows: Rc::default(),
            list_scroll: UniformListScrollHandle::new(),
            _sub: sub,
        }
    }

    pub fn filter_focus(&self, cx: &App) -> FocusHandle {
        self.filter.focus_handle(cx)
    }

    fn visible(&self, cx: &App) -> Vec<usize> {
        let q = self.filter.read(cx).value().trim().to_lowercase();
        self.entries
            .as_ref()
            .map(|es| {
                (0..es.len())
                    .filter(|&i| q.is_empty() || es[i].label.to_lowercase().contains(&q))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        let Some(es) = &self.entries else {
            return Vec::new();
        };
        self.selected.iter().map(|&i| es[i].path.clone()).collect()
    }

    pub fn add_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        window.close_dialog(cx);
        (self.on_add)(paths, window, cx);
    }

    fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let n = self.selected.len();
        h_flex()
            .pt_2()
            .gap_2()
            .justify_end()
            .child(
                Button::new("scan-cancel")
                    .small()
                    .label("取消")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("scan-add")
                    .small()
                    .primary()
                    .label(if n == 0 {
                        "添加".to_string()
                    } else {
                        format!("添加 {n} 个仓库")
                    })
                    .disabled(n == 0)
                    .on_click(cx.listener(|this, _, window, cx| this.add_selected(window, cx))),
            )
    }

    fn toggle(&mut self, i: usize, cx: &mut Context<Self>) {
        if self.entries.as_ref().is_some_and(|es| es[i].already_open) {
            return;
        }
        if !self.selected.remove(&i) {
            self.selected.insert(i);
        }
        cx.notify();
    }

    /// 整行都是点击区域;复选框只负责显示,点它时事件冒泡到行上,只切换一次。
    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let Some(entries) = &self.entries else {
            return Vec::new();
        };
        let rows = self.visible_rows.clone();
        rows[range.start.min(rows.len())..range.end.min(rows.len())]
            .iter()
            .map(|&i| {
                let e = &entries[i];
                let checked = self.selected.contains(&i) || e.already_open;
                h_flex()
                    .id(("scan-row", i))
                    .px_2()
                    .h(px(ROW_HEIGHT))
                    .gap_2()
                    .rounded_sm()
                    .text_sm()
                    .when(e.already_open, |d| d.text_color(theme.muted_foreground))
                    .when(!e.already_open, |d| {
                        d.cursor_pointer()
                            .hover(|s| s.bg(theme.list_hover))
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle(i, cx)))
                    })
                    .child(
                        Checkbox::new(("scan-check", i))
                            .checked(checked)
                            .disabled(e.already_open),
                    )
                    .child(div().flex_1().truncate().child(e.label.clone()))
                    .when(e.already_open, |d| d.child(div().text_xs().child("已打开")))
                    .test_support()
                    .into_any_element()
            })
            .collect()
    }

    fn set_all_visible(&mut self, on: bool, cx: &mut Context<Self>) {
        let visible = self.visible(cx);
        let Some(es) = &self.entries else { return };
        for i in visible {
            if es[i].already_open {
                continue;
            }
            if on {
                self.selected.insert(i);
            } else {
                self.selected.remove(&i);
            }
        }
        cx.notify();
    }
}

impl Render for ScanView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let header = h_flex()
            .gap_2()
            .text_sm()
            .text_color(theme.muted_foreground)
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .child(format!("扫描 {}", self.root.display())),
            );

        let Some(entries) = &self.entries else {
            return v_flex()
                .gap_3()
                .child(header)
                .child(
                    h_flex()
                        .gap_2()
                        .py_6()
                        .justify_center()
                        .child(gpui::component::spinner::Spinner::new().small())
                        .child(format!("正在查找 Git 仓库(最多 {DEFAULT_MAX_DEPTH} 层)…")),
                )
                .child(self.footer(cx))
                .into_any_element();
        };
        if entries.is_empty() {
            return v_flex()
                .gap_3()
                .child(header)
                .child(
                    div()
                        .py_6()
                        .text_center()
                        .text_color(theme.muted_foreground)
                        .child(format!(
                            "这个目录下 {DEFAULT_MAX_DEPTH} 层内没有找到 Git 仓库"
                        )),
                )
                .child(self.footer(cx))
                .into_any_element();
        }

        let visible = self.visible(cx);
        let open_count = entries.iter().filter(|e| e.already_open).count();
        let row_count = visible.len();
        self.visible_rows = Rc::new(visible);

        v_flex()
            .gap_2()
            .child(header.child(format!(
                "找到 {} 个{}",
                entries.len(),
                if open_count > 0 {
                    format!(",{open_count} 个已打开")
                } else {
                    String::new()
                }
            )))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&self.filter).small().cleanable(true)),
                    )
                    .child(
                        Button::new("scan-select-all")
                            .xsmall()
                            .ghost()
                            .label("全选")
                            .on_click(cx.listener(|this, _, _, cx| this.set_all_visible(true, cx))),
                    )
                    .child(
                        Button::new("scan-select-none")
                            .xsmall()
                            .ghost()
                            .label("全不选")
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_all_visible(false, cx)),
                            ),
                    ),
            )
            .child(
                uniform_list(
                    "scan-list",
                    row_count,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        this.render_rows(range, cx)
                    }),
                )
                .track_scroll(&self.list_scroll)
                .h(px((row_count as f32 * ROW_HEIGHT).min(LIST_MAX_HEIGHT))),
            )
            .child(self.footer(cx))
            .into_any_element()
    }
}
