use super::*;

#[test]
fn custom_names_trim_unicode_and_empty_restores_automatic_title() {
    assert_eq!(normalize_name("  后端 API 🚀  "), Some("后端 API 🚀".into()));
    assert_eq!(normalize_name(" \t\n\u{3000}"), None);
    let long = "项目".repeat(200);
    assert_eq!(normalize_name(&long), Some(long));
}

#[cfg(feature = "gpui-test-support")]
mod interaction {
    use super::*;
    use gpui::{
        IntoElement, Modifiers, Render, StatefulInteractiveElement as _, TestAppContext,
        VisualTestContext, div, point,
    };
    use gpui_component::Root;

    #[derive(Default)]
    struct Probe {
        name: Option<String>,
        submissions: usize,
        parent_clicks: usize,
    }

    impl Render for Probe {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .id("pane-header-probe")
                .on_click(cx.listener(|this, _, _, _| this.parent_clicks += 1))
                .child(rename_button(42, cx.theme().foreground, cx).on_click(cx.listener(
                    |this, _, window, cx| {
                        cx.stop_propagation();
                        let current = this.name.clone().unwrap_or_else(|| "shell".into());
                        let probe = cx.entity().downgrade();
                        show_rename_dialog(current, window, cx, move |name, _, cx| {
                            if let Some(probe) = probe.upgrade() {
                                probe.update(cx, |probe, cx| {
                                    probe.name = name;
                                    probe.submissions += 1;
                                    cx.notify();
                                });
                            }
                        });
                    },
                )))
                .children(Root::render_dialog_layer(window, cx))
        }
    }

    fn setup(cx: &mut TestAppContext) -> (gpui::Entity<Probe>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            super::super::super::init(cx);
            cx.set_reduce_motion(true);
        });
        let mut probe = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| Probe::default());
            probe = Some(view.clone());
            Root::new(view, window, cx)
        });
        (probe.unwrap(), cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        draw(cx);
        let bounds = cx.debug_bounds(selector).expect("control must have a hit target");
        cx.simulate_click(
            point(
                bounds.origin.x + bounds.size.width * 0.5,
                bounds.origin.y + bounds.size.height * 0.5,
            ),
            Modifiers::default(),
        );
        draw(cx);
        draw(cx);
    }

    #[gpui::test]
    fn pane_rename_button_saves_unicode_only_after_confirmation(cx: &mut TestAppContext) {
        let (probe, cx) = setup(cx);
        click(cx, "pane-rename-42");
        cx.simulate_input("后端 API 🚀");
        probe.read_with(cx, |probe, _| {
            assert_eq!(probe.name, None);
            assert_eq!(probe.submissions, 0);
            assert_eq!(probe.parent_clicks, 0);
        });
        click(cx, "confirm-dialog-ok");
        probe.read_with(cx, |probe, _| {
            assert_eq!(probe.name.as_deref(), Some("后端 API 🚀"));
            assert_eq!(probe.submissions, 1);
        });
    }

    #[gpui::test]
    fn pane_rename_enter_saves_and_escape_or_cancel_discards(cx: &mut TestAppContext) {
        let (probe, cx) = setup(cx);
        click(cx, "pane-rename-42");
        cx.simulate_input("original");
        cx.simulate_keystrokes("enter");
        draw(cx);
        assert_eq!(probe.read_with(cx, |probe, _| probe.name.clone()), Some("original".into()));
        click(cx, "pane-rename-42");
        cx.simulate_input("discarded");
        cx.simulate_keystrokes("escape");
        draw(cx);
        click(cx, "pane-rename-42");
        cx.simulate_input("also discarded");
        click(cx, "confirm-dialog-cancel");
        probe.read_with(cx, |probe, _| {
            assert_eq!(probe.name.as_deref(), Some("original"));
            assert_eq!(probe.submissions, 1);
        });
    }

    #[gpui::test]
    fn pane_rename_whitespace_restores_automatic_title(cx: &mut TestAppContext) {
        let (probe, cx) = setup(cx);
        probe.update(cx, |probe, _| probe.name = Some("custom".into()));
        click(cx, "pane-rename-42");
        cx.simulate_input("   ");
        click(cx, "confirm-dialog-ok");
        assert_eq!(probe.read_with(cx, |probe, _| probe.name.clone()), None);
    }
}
