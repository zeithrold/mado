use gpui::{App, MouseButton, Window, div, prelude::*, px, rgb, svg};

use mado_icons::LucideIcon;

const TITLE_BAR_HEIGHT: f32 = 48.0;
const ICON_BUTTON_SIZE: f32 = 32.0;
const ICON_SIZE: f32 = 16.0;

pub fn title_bar() -> impl IntoElement {
    div()
        .h(px(TITLE_BAR_HEIGHT))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .px_2()
        .bg(rgb(0x00ec_ecec))
        .on_mouse_down(MouseButton::Left, |_, window, _| {
            window.start_window_move();
        })
        .child(icon_button("menu-button", LucideIcon::Menu, |_, _| {}))
}

fn icon_button(
    id: &'static str,
    icon: LucideIcon,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .size(px(ICON_BUTTON_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .hover(|style| style.bg(rgb(0x00dc_dcdc)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
        })
        .on_click(move |_, window, cx| {
            on_click(window, cx);
        })
        .child(
            svg()
                .path(icon.path())
                .size(px(ICON_SIZE))
                .text_color(rgb(0x003a_3a3a)),
        )
}
