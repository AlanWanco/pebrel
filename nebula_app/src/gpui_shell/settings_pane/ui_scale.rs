use super::*;
use crate::i18n::Message;

impl SettingsPane {
    pub(super) fn set_ui_scale(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(scale) = nebula_settings::UiScale::from_settings(value) else { return };
        if let Err(error) = self.try_persist(&[("ui_scale", scale.settings_value().into())], cx) {
            self.sync_select("ui_scale", self.runtime.ui_scale.settings_value(), window, cx);
            let language = crate::gpui_shell::config::ui_language(cx);
            crate::gpui_shell::toast::toast(
                window,
                cx,
                crate::gpui_shell::toast::ToastKind::Warning,
                language
                    .format(Message::SettingsUiScaleSaveFailed, &[("error", &error.to_string())]),
            );
        } else {
            // All Root views consume the same theme REM; do not modify OS DPI or
            // the terminal font preference. New windows use the cached scale too.
            crate::gpui_shell::theme::apply_chrome_theme(cx);
            cx.refresh_windows();
        }
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::{Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, point};
    use gpui_component::{Root, Theme};

    #[gpui::test]
    fn ui_scale_setting_is_reachable_at_1080p_and_in_narrow_windows(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.set_reduce_motion(true);
            let mut settings = crate::gpui_shell::config::Settings::load(ThemeName::Nord);
            settings.ui_language = crate::display::UiLanguage::EnUs;
            settings.ui_scale = nebula_settings::UiScale::default();
            cx.set_global(settings);
        });
        let mut pane = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| SettingsPane::new(window, cx));
            view.update(cx, |pane, _| {
                pane.runtime = RuntimeSettings::from_raw(&nebula_settings::RawSettings::default());
                pane.active_section = 1;
            });
            pane = Some(view.clone());
            Root::new(view, window, cx)
        });
        let pane = pane.unwrap();
        for (width, height, percent) in
            [(1920.0, 1080.0, "100"), (1920.0, 1080.0, "150"), (1080.0, 720.0, "200")]
        {
            let scale = nebula_settings::UiScale::from_settings(percent).unwrap();
            cx.simulate_resize(gpui::size(px(width), px(height)));
            cx.update(|window, cx| {
                cx.global_mut::<crate::gpui_shell::config::Settings>().ui_scale = scale;
                Theme::global_mut(cx).font_size =
                    px(crate::gpui_shell::ui_scale::BASE_REM * scale.factor());
                pane.update(cx, |pane, cx| {
                    pane.runtime.ui_scale = scale;
                    cx.notify();
                });
                window.refresh();
            });
            cx.run_until_parked();
            cx.update(|window, cx| {
                let _ = window.draw(cx);
            });
            let mut bounds =
                cx.debug_bounds("settings-select-ui_scale").expect("scale control in Interface");
            let nav = cx.debug_bounds("settings-navigation").expect("settings navigation");
            assert!(bounds.left() >= nav.right(), "control must not overlap navigation");
            assert!(
                bounds.right() <= px(width),
                "control must remain inside the window at {percent}%"
            );
            // At large scales the Interface group is below the initial viewport.
            // Exercise the real settings scroll container before clicking it.
            for _ in 0..16 {
                if bounds.top() >= px(0.0) && bounds.bottom() <= px(height) {
                    break;
                }
                let delta = if bounds.bottom() > px(height) { -400.0 } else { 400.0 };
                cx.simulate_event(ScrollWheelEvent {
                    position: point(px(width * 0.75), px(height * 0.75)),
                    delta: ScrollDelta::Pixels(point(px(0.0), px(delta))),
                    modifiers: Modifiers::default(),
                    touch_phase: TouchPhase::Moved,
                });
                cx.run_until_parked();
                cx.update(|window, cx| {
                    let _ = window.draw(cx);
                });
                bounds = cx
                    .debug_bounds("settings-select-ui_scale")
                    .expect("scale control remains rendered after scrolling");
            }
            assert!(
                bounds.top() >= px(0.0) && bounds.bottom() <= px(height),
                "scale control is not reachable at {percent}%: bounds={bounds:?}, viewport={width}x{height}"
            );
            cx.simulate_click(bounds.center(), Modifiers::default());
            cx.run_until_parked();
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
            assert_eq!(pane.read_with(cx, |pane, _| pane.runtime.ui_scale), scale);
        }
    }
}
