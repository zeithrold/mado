mod index;
mod title_bar;

use gpui::{
    App, Application, Bounds, Context, Menu, MenuItem, Render, TitlebarOptions, Window,
    WindowBounds, WindowOptions, actions, div, prelude::*, px, rgb, size,
};
use gpui_component::Root;

use mado_icons::LucideAssets;

actions!(
    mado_demo,
    [
        #[allow(clippy::derive_partial_eq_without_eq)]
        About,
        #[allow(clippy::derive_partial_eq_without_eq)]
        Quit,
    ]
);

struct AboutWindow;

impl Render for AboutWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .size_full()
            .justify_center()
            .items_center()
            .bg(rgb(0x00f5_f5f5))
            .text_color(rgb(0x001a_1a1a))
            .child("Mado Demo")
            .child(format!("Version {}", env!("CARGO_PKG_VERSION")))
    }
}

fn main() {
    Application::new()
        .with_assets(LucideAssets::new())
        .run(|cx: &mut App| {
            cx.activate(true);
            gpui_component::init(cx);

            #[cfg(target_os = "macos")]
            setup_menus(cx);

            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(400.0), px(640.0)), cx);
            let _ = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Mado Demo".into()),
                        ..Default::default()
                    }),
                    is_resizable: false,
                    is_minimizable: false,
                    window_min_size: Some(size(px(400.0), px(640.0))),
                    ..Default::default()
                },
                |window, cx| {
                    let index = cx.new(|_| index::IndexPage::new());
                    cx.new(|cx| Root::new(index, window, cx))
                },
            );
        });
}

#[cfg(target_os = "macos")]
fn setup_menus(cx: &mut App) {
    cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
    cx.on_action(open_about);
    cx.set_menus(vec![Menu {
        name: "Mado Demo".into(),
        items: vec![
            MenuItem::action("About Mado Demo", About),
            MenuItem::separator(),
            MenuItem::action("Quit Mado Demo", Quit),
        ],
    }]);
}

#[cfg(target_os = "macos")]
fn open_about(_: &About, cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(360.0), px(180.0)), cx);
    let _ = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |_, cx| cx.new(|_| AboutWindow),
    );
}
