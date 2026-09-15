//! Chrome scaling uses GPUI's REM layout, not a synthetic display scale factor.
//! Measured window/pointer coordinates and terminal grid metrics remain logical pixels.

use gpui::{App, Rems, rems};

/// The product's existing unscaled component root font size.
pub(crate) const BASE_REM: f32 = 14.0;

pub(crate) fn factor(cx: &App) -> f32 {
    cx.try_global::<super::config::Settings>().map_or(1.0, |settings| settings.ui_scale.factor())
}

/// A chrome design dimension expressed in pixels at 100%, resolved by the root REM.
/// Do not use for measured positions, terminal font sizes, or physical-pixel hairlines.
pub(crate) fn ui(value: f32) -> Rems {
    rems(value / BASE_REM)
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::{
        AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
        Styled as _, TestAppContext, Window, div, px,
    };
    use gpui_component::{Root, Theme};

    struct Probe;
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(
                    div()
                        .id("scaled-control")
                        .debug_selector(|| "scaled-control".into())
                        .w(ui(120.0))
                        .h(ui(32.0))
                        .text_size(ui(14.0))
                        .child("Interface"),
                )
                .child(
                    div()
                        .id("terminal-metric")
                        .debug_selector(|| "terminal-metric".into())
                        .w(px(120.0))
                        .h(px(32.0)),
                )
        }
    }

    #[gpui::test]
    fn ui_scale_changes_real_layout_but_not_dpi_or_terminal_pixels(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let (_, cx) = cx.add_window_view(|window, cx| {
            let probe = cx.new(|_| Probe);
            Root::new(probe, window, cx)
        });
        for scale in [1.0, 1.25, 1.5, 2.0, 1.0] {
            cx.update(|window, cx| {
                window.set_scale_factor(1.5);
                Theme::global_mut(cx).font_size = px(BASE_REM * scale);
                window.refresh();
            });
            cx.update(|window, cx| {
                let _ = window.draw(cx);
            });
            let scaled = cx.debug_bounds("scaled-control").unwrap();
            let fixed = cx.debug_bounds("terminal-metric").unwrap();
            assert!((f32::from(scaled.size.width) - 120.0 * scale).abs() < 1.0);
            assert!((f32::from(scaled.size.height) - 32.0 * scale).abs() < 1.0);
            assert!((f32::from(fixed.size.width) - 120.0).abs() < 1.0);
            cx.update(|window, _| assert_eq!(window.scale_factor(), 1.5));
        }
    }
}
