use std::sync::{Arc, OnceLock};
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, Context, Image, ImageFormat, MouseButton, ObjectFit, Render, Task,
    Window, div, img, prelude::*, px, rgb,
};
use gpui_component::{ActiveTheme, Icon, IconName, Sizable, Size, StyledExt, h_flex, v_flex};

use crate::title_bar;

const AVATAR_SIZE: f32 = 128.0;
const HOVER_BG_SCALE: f32 = 0.4;
const HOVER_BG_MAX_SCALE: f32 = 0.75 + HOVER_BG_SCALE;
const AVATAR_CONTAINER_SIZE: f32 = AVATAR_SIZE * HOVER_BG_MAX_SCALE;
const LAUNCH_BUTTON_WIDTH: f32 = 300.0;
const LAUNCH_BUTTON_HEIGHT: f32 = 64.0;
const DROPDOWN_TRIGGER_SIZE: f32 = 32.0;
const TOOLTIP_DELAY: Duration = Duration::from_millis(150);
const TOOLTIP_ANIM_DURATION: Duration = Duration::from_millis(160);
const TOOLTIP_START_GAP: f32 = 2.0;
const TOOLTIP_END_GAP: f32 = 8.0;
const HOVER_ANIM_DURATION: Duration = Duration::from_millis(220);

#[derive(Clone, Copy, PartialEq, Eq)]
enum HoverPhase {
    Hidden,
    In,
    Out,
}

static STEVE_IMAGE: OnceLock<Arc<Image>> = OnceLock::new();

fn steve_image() -> Arc<Image> {
    Arc::clone(STEVE_IMAGE.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../assets/steve.png").to_vec(),
        ))
    }))
}

fn ease_out_cubic(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(3)
}

fn ease_out_quint(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(5)
}

pub struct IndexPage {
    avatar_phase: HoverPhase,
    avatar_task: Option<Task<()>>,
    tooltip_phase: HoverPhase,
    tooltip_task: Option<Task<()>>,
}

impl IndexPage {
    pub const fn new() -> Self {
        Self {
            avatar_phase: HoverPhase::Hidden,
            avatar_task: None,
            tooltip_phase: HoverPhase::Hidden,
            tooltip_task: None,
        }
    }
}

impl Render for IndexPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full().child(title_bar::title_bar()).child(
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_6()
                .child(self.account_avatar(cx))
                .child(Self::launch_button(cx)),
        )
    }
}

impl IndexPage {
    fn account_avatar(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .id("account-avatar")
            .relative()
            .size(px(AVATAR_CONTAINER_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_hover(cx.listener(|this, hovered, _, cx| {
                if *hovered {
                    this.avatar_phase = HoverPhase::In;
                    this.avatar_task = None;
                    this.tooltip_task = None;
                    if this.tooltip_phase == HoverPhase::Hidden {
                        this.tooltip_task = Some(cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(TOOLTIP_DELAY).await;
                            let _ = this.update(cx, |this, cx| {
                                this.tooltip_phase = HoverPhase::In;
                                cx.notify();
                            });
                        }));
                    } else {
                        this.tooltip_phase = HoverPhase::In;
                    }
                } else {
                    if this.avatar_phase != HoverPhase::Hidden {
                        this.avatar_phase = HoverPhase::Out;
                        this.avatar_task = Some(cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(HOVER_ANIM_DURATION).await;
                            let _ = this.update(cx, |this, cx| {
                                this.avatar_phase = HoverPhase::Hidden;
                                cx.notify();
                            });
                        }));
                    }
                    this.tooltip_task = None;
                    if this.tooltip_phase != HoverPhase::Hidden {
                        this.tooltip_phase = HoverPhase::Out;
                        this.tooltip_task = Some(cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(TOOLTIP_ANIM_DURATION).await;
                            let _ = this.update(cx, |this, cx| {
                                this.tooltip_phase = HoverPhase::Hidden;
                                cx.notify();
                            });
                        }));
                    }
                }
                cx.notify();
            }))
            .when(self.avatar_phase != HoverPhase::Hidden, |this| {
                let entering = self.avatar_phase == HoverPhase::In;
                let anim_id = if entering {
                    "avatar-hover-in"
                } else {
                    "avatar-hover-out"
                };
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .flex_none()
                                .rounded_xl()
                                .bg(rgb(0x00e8_e8e8))
                                .with_animation(
                                    anim_id,
                                    Animation::new(HOVER_ANIM_DURATION),
                                    move |el, delta| {
                                        let phase = if entering { delta } else { 1.0 - delta };
                                        let size_progress = ease_out_cubic(phase);
                                        let opacity_progress = ease_out_quint(phase);
                                        let scale = HOVER_BG_SCALE.mul_add(size_progress, 0.75);
                                        let size = AVATAR_SIZE * scale;
                                        el.opacity(opacity_progress).size(px(size))
                                    },
                                ),
                        ),
                )
            })
            .child(
                div()
                    .flex_none()
                    .size(px(AVATAR_SIZE))
                    .rounded_lg()
                    .overflow_hidden()
                    .child(img(steve_image()).size_full().object_fit(ObjectFit::Cover)),
            )
            .when(self.tooltip_phase != HoverPhase::Hidden, |this| {
                this.child(Self::avatar_tooltip(self.tooltip_phase))
            })
    }

    fn avatar_tooltip(phase: HoverPhase) -> impl IntoElement {
        let entering = phase == HoverPhase::In;
        let anim_id = if entering {
            "avatar-tooltip-in"
        } else {
            "avatar-tooltip-out"
        };

        div()
            .absolute()
            .bottom_full()
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(0x0033_3333))
                    .text_color(rgb(0x00f5_f5f5))
                    .text_xs()
                    .whitespace_nowrap()
                    .child("Switch Account"),
            )
            .with_animation(
                anim_id,
                Animation::new(TOOLTIP_ANIM_DURATION),
                move |el, delta| {
                    let phase = if entering { delta } else { 1.0 - delta };
                    let slide_progress = ease_out_cubic(phase);
                    let opacity_progress = ease_out_quint(phase);
                    let gap =
                        TOOLTIP_START_GAP + (TOOLTIP_END_GAP - TOOLTIP_START_GAP) * slide_progress;
                    el.mb(px(gap)).opacity(opacity_progress)
                },
            )
    }

    fn launch_button(cx: &Context<Self>) -> impl IntoElement {
        let primary_fg = cx.theme().primary_foreground;
        let secondary_fg = primary_fg.opacity(0.75);

        h_flex()
            .id("launch-button")
            .w(px(LAUNCH_BUTTON_WIDTH))
            .h(px(LAUNCH_BUTTON_HEIGHT))
            .rounded_xl()
            .bg(cx.theme().primary)
            .text_color(primary_fg)
            .items_center()
            .cursor_pointer()
            .on_click(|_, _, _| {})
            .child(
                v_flex()
                    .flex_1()
                    .px_4()
                    .gap_0p5()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_lg()
                            .font_semibold()
                            .text_center()
                            .child("Launch"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(secondary_fg)
                            .text_center()
                            .child("Minecraft 1.20.8"),
                    ),
            )
            .child(
                div()
                    .w(px(1.0))
                    .h(px(LAUNCH_BUTTON_HEIGHT - 16.0))
                    .bg(primary_fg.opacity(0.25)),
            )
            .child(
                div()
                    .id("launch-dropdown-trigger")
                    .w(px(DROPDOWN_TRIGGER_SIZE))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .on_click(|_, _, _| {})
                    .child(
                        Icon::new(IconName::ChevronDown)
                            .text_color(primary_fg)
                            .with_size(Size::Small),
                    ),
            )
    }
}
