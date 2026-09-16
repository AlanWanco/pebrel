use super::*;

/// 侧栏拖宽热区的宽度。热区中心由 [`sidebar_resize_offset_for`] 决定。
pub(super) const SIDEBAR_RESIZE_HANDLE_WIDTH: f32 = 6.0;

/// 侧栏槽位右缘到「用户眼里那条分界」的距离，热区与拖拽换算都用它。
///
/// 两种形态的分界不在同一个位置：主题画了竖线（Nord 这类铺满布局，卡缝 0 +
/// 1px 竖线）时分界就是那条线，它贴在槽位右缘上；没画线的浮起圆角卡（卡缝 8
/// + 无竖线）里分界是卡缝右侧的卡可见左缘。原来这里写死 8.0 只对后者成立，
/// Nord 成为出厂默认之后热区整整偏右 7.5px——鼠标停在线上不变形，得往右挪
/// 半个字符宽才拖得动。
pub(super) fn sidebar_resize_offset_for(divider: f32, gutter: f32) -> f32 {
    if divider > 0.0 { divider * 0.5 } else { gutter }
}

pub(super) fn sidebar_resize_visual_offset(cx: &App) -> f32 {
    let card = crate::gpui_shell::theme::PaneCardStyle::current(cx);
    let scale = crate::gpui_shell::ui_scale::factor(cx);
    sidebar_resize_offset_for(card.divider, card.margin.left * scale)
}

/// 标题栏里的文件树 / Git 工具必须同时挡住原生拖窗命中和父级拖拽起手。
/// `occlude` 只屏蔽后方 hitbox，不会阻止 MouseDown 向 `TitleBar` 冒泡。
pub(super) fn title_bar_panel_controls() -> gpui::Div {
    h_flex()
        .h_full()
        .items_center()
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}
