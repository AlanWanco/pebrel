//! Pane-owned titles and a scoped rename dialog. No PTY input or tab-name mutation.

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Hsla, InteractiveElement as _, ParentElement as _, Styled as _,
    Window,
};

use super::{NebulaWorkspace, WorkspaceTab};
use crate::gpui_shell::prelude::*;
use crate::i18n::Message;

pub(super) fn normalize_name(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub(super) fn rename_button(pane_id: u64, ink: Hsla, cx: &App) -> Button {
    let language = crate::gpui_shell::config::ui_language(cx);
    Button::new(("pane-rename", pane_id as usize))
        .debug_selector(move || format!("pane-rename-{pane_id}"))
        .icon(Icon::default().path(crate::gpui_shell::assets::nav::PENCIL).text_color(ink))
        .ghost()
        .xsmall()
        .tooltip(language.text(Message::PaneRenameTitle))
}

impl NebulaWorkspace {
    pub(super) fn pane_custom_name(&self, pane_id: u64) -> Option<&str> {
        let tab = self.tab_of_pane(pane_id)?;
        let WorkspaceTab::Terminal { panes, .. } = self.tabs.get(tab)? else { return None };
        panes.iter().find(|pane| pane.id == pane_id)?.custom_name.as_deref()
    }

    pub(super) fn begin_pane_rename(
        &mut self,
        pane_id: u64,
        current: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab_of_pane(pane_id).is_none() {
            return;
        }
        self.pane_drag = None;
        let workspace = cx.entity().downgrade();
        show_rename_dialog(current, window, cx, move |name, _, cx| {
            let Some(workspace) = workspace.upgrade() else { return };
            workspace.update(cx, |workspace, cx| {
                // Resolve the stable pane ID at commit time, never a captured tab index.
                let Some(tab) = workspace.tab_of_pane(pane_id) else { return };
                let Some(WorkspaceTab::Terminal { panes, .. }) = workspace.tabs.get_mut(tab) else {
                    return;
                };
                let Some(pane) = panes.iter_mut().find(|pane| pane.id == pane_id) else { return };
                pane.custom_name = name;
                cx.notify();
            });
        });
    }
}

fn show_rename_dialog(
    current: String,
    window: &mut Window,
    cx: &mut App,
    submit: impl Fn(Option<String>, &mut Window, &mut App) + 'static,
) {
    let language = crate::gpui_shell::config::ui_language(cx);
    let input = cx.new(|cx| InputState::new(window, cx));
    input.update(cx, |state, cx| {
        let len = current.len();
        state.set_value(current, window, cx);
        state.set_selected_range(0..len, cx);
    });
    let focus_input = input.clone();
    let submit = Rc::new(submit);
    window.open_dialog(cx, move |dialog, window, _| {
        let input_on_ok = input.clone();
        let submit = submit.clone();
        confirm_dialog(
            dialog,
            window,
            language.text(Message::PaneRenameTitle),
            language.text(Message::PaneRenameHint),
            language.text(Message::CommonSave),
            language.text(Message::CommonCancel),
            ButtonVariant::Primary,
        )
        .child(Input::new(&input).w_full())
        .on_ok(move |_, window, cx| {
            submit(normalize_name(&input_on_ok.read(cx).value()), window, cx);
            true
        })
    });
    focus_input.update(cx, |state, cx| state.focus(window, cx));
}

#[cfg(test)]
mod tests;
