//! Persistent mixer controls. Rendering reads cached values; device changes are explicit actions.
use super::{
    VoiceApp, roll,
    theme::Theme,
    views::{Controls, button, label},
};
use gpui::{
    AnyElement, Bounds, Context, Div, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    Pixels, Window, canvas, div, prelude::*, px, rgb, rgba,
};
use std::{cell::Cell, rc::Rc};
use voice::model::TrackRole;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Channel {
    Voice,
    Reference,
    Accompaniment,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Selector {
    Input,
    Output,
    Channel,
}
#[derive(Clone, Copy)]
struct Drag {
    channel: Channel,
    top: f32,
    travel: f32,
}
#[derive(Clone, Copy)]
struct MenuState {
    selector: Selector,
    x: f32,
    y: f32,
    width: f32,
    index: usize,
}
#[derive(Default)]
pub(super) struct MixerUi {
    drag: Option<Drag>,
    menu: Option<MenuState>,
    menu_scroll: gpui::ScrollHandle,
}
impl MixerUi {
    pub(super) fn dismiss(&mut self) {
        self.drag = None;
        self.menu = None;
    }
    pub(super) fn close_menu(&mut self) -> bool {
        self.menu.take().is_some()
    }
}

fn gain_position(gain: f32) -> f32 {
    if gain <= 1.0 {
        gain.max(0.0).cbrt() * 0.8
    } else {
        0.8 + (gain.min(2.0) - 1.0) * 0.2
    }
}
fn position_gain(position: f32) -> f32 {
    let p = position.clamp(0.0, 1.0);
    if p <= 0.8 {
        (p / 0.8).powi(3)
    } else {
        1.0 + (p - 0.8) / 0.2
    }
}
fn gain_label(gain: f32) -> String {
    if gain <= 0.00001 {
        "−∞ dB".into()
    } else {
        format!("{:+.1} dB", 20.0 * gain.log10())
    }
}
fn apply_gain(app: &mut VoiceApp, channel: Channel, gain: f32) {
    match channel {
        Channel::Voice => app.set_voice_gain(gain.clamp(0.0, 2.0)),
        Channel::Reference => app.set_mix_gain(TrackRole::Target, gain.clamp(0.0, 2.0)),
        Channel::Accompaniment => app.set_mix_gain(TrackRole::Backing, gain.clamp(0.0, 2.0)),
    }
}
fn current_gain(app: &VoiceApp, channel: Channel) -> f32 {
    match channel {
        Channel::Voice => app.doc().settings.monitor_gain,
        Channel::Reference => app.doc().settings.target_gain,
        Channel::Accompaniment => app.doc().settings.backing_gain,
    }
}
pub(super) fn move_fader(app: &mut VoiceApp, event: &MouseMoveEvent) -> bool {
    let Some(drag) = app.mixer_ui.drag else {
        return false;
    };
    if event.pressed_button != Some(MouseButton::Left) {
        app.mixer_ui.drag = None;
        return true;
    }
    let y: f32 = event.position.y.into();
    apply_gain(
        app,
        drag.channel,
        position_gain(1.0 - (y - drag.top) / drag.travel.max(1.0)),
    );
    true
}
pub(super) fn end_drag(app: &mut VoiceApp) {
    app.mixer_ui.drag = None;
}

struct FaderSpec {
    channel: Channel,
    title: &'static str,
    gain: f32,
    peak: f32,
}

fn fader(app: &VoiceApp, spec: FaderSpec, controls: Controls, cx: &Context<VoiceApp>) -> Div {
    let FaderSpec {
        channel,
        title,
        gain,
        peak,
    } = spec;
    let theme = controls.theme;
    let enabled = controls.enabled;
    let color = match channel {
        Channel::Voice => theme.live,
        Channel::Reference => theme.accent,
        Channel::Accompaniment => theme.waveform,
    };
    let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
    let paint_bounds = bounds.clone();
    let click_bounds = bounds.clone();
    let active = app
        .mixer_ui
        .drag
        .is_some_and(|drag| drag.channel == channel);
    let canvas = canvas(
        move |b, _, _| {
            paint_bounds.set(b);
        },
        move |b, _, window, _| {
            let x: f32 = b.origin.x.into();
            let y: f32 = b.origin.y.into();
            let w: f32 = b.size.width.into();
            let h: f32 = b.size.height.into();
            let top = y + 8.0;
            let travel = (h - 16.0).max(1.0);
            let rail = x + w * 0.42;
            roll::rect(window, rail - 2.0, top, 4.0, travel, theme.separator, 2.0);
            for value in [0.0, 0.1, 0.3162, 1.0, 2.0] {
                let at = top + (1.0 - gain_position(value)) * travel;
                roll::rect(
                    window,
                    rail - 13.0,
                    at,
                    8.0,
                    1.0,
                    if value == 1.0 {
                        theme.text
                    } else {
                        theme.separator
                    },
                    0.0,
                );
            }
            let thumb = top + (1.0 - gain_position(gain)) * travel;
            roll::rect(
                window,
                rail - 1.0,
                thumb,
                2.0,
                (top + travel - thumb).max(0.0),
                if enabled { color } else { theme.separator },
                1.0,
            );
            let meter_x = x + w * 0.79;
            roll::rect(window, meter_x, top, 5.0, travel, theme.separator, 2.0);
            let meter = ((20.0 * peak.max(0.00001).log10() + 60.0) / 60.0).clamp(0.0, 1.0);
            if meter > 0.0 {
                roll::rect(
                    window,
                    meter_x,
                    top + (1.0 - meter) * travel,
                    5.0,
                    meter * travel,
                    if peak >= 0.98 {
                        theme.recording
                    } else if peak > 0.7 {
                        theme.live
                    } else {
                        theme.meter
                    },
                    2.0,
                );
            }
            roll::rect(
                window,
                rail - 15.0,
                thumb - 6.0,
                30.0,
                12.0,
                if active {
                    theme.accent
                } else {
                    theme.secondary
                },
                3.0,
            );
            roll::rect(
                window,
                rail - 14.0,
                thumb - 5.0,
                28.0,
                10.0,
                theme.control,
                2.0,
            );
            roll::rect(
                window,
                rail - 8.0,
                thumb - 0.5,
                16.0,
                1.0,
                if enabled { color } else { theme.secondary },
                0.0,
            );
        },
    )
    .size_full();
    div()
        .w(px(94.))
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .child(
            div()
                .h(px(34.))
                .w_full()
                .text_center()
                .text_size(px(11.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            div()
                .id(match channel {
                    Channel::Voice => "voice-fader",
                    Channel::Reference => "reference-fader",
                    Channel::Accompaniment => "accompaniment-fader",
                })
                .key_context("VoiceControl")
                .w_full()
                .h(px(150.))
                .rounded(px(5.))
                .when(!enabled, |d| d.opacity(0.4))
                .when(enabled, |d| {
                    d.tab_index(0).cursor_pointer().focus(move |s| {
                        s.bg(rgba(0x80808010))
                            .border_1()
                            .border_color(rgb(theme.accent))
                    })
                })
                .child(canvas)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |a, event: &MouseDownEvent, window, c| {
                        if !enabled {
                            return;
                        }
                        let b = click_bounds.get();
                        let y: f32 = b.origin.y.into();
                        let h: f32 = b.size.height.into();
                        let py: f32 = event.position.y.into();
                        a.mixer_ui.menu = None;
                        a.mixer_ui.drag = Some(Drag {
                            channel,
                            top: y + 8.0,
                            travel: (h - 16.0).max(1.0),
                        });
                        if event.click_count >= 2 {
                            a.mixer_ui.drag = None;
                            apply_gain(a, channel, 1.0);
                        } else {
                            apply_gain(
                                a,
                                channel,
                                position_gain(1.0 - (py - y - 8.0) / (h - 16.0).max(1.0)),
                            );
                        }
                        window.prevent_default();
                        c.stop_propagation();
                        c.notify();
                    }),
                )
                .on_key_down(cx.listener(move |a, event: &KeyDownEvent, w, c| {
                    if !enabled {
                        return;
                    }
                    let key = event.keystroke.key.as_str();
                    let value = current_gain(a, channel);
                    let step = if event.keystroke.modifiers.shift {
                        0.1
                    } else {
                        1.0
                    };
                    let next = match key {
                        "up" | "right" => Some(if value <= 0.00001 {
                            0.001
                        } else {
                            value * 10.0f32.powf(step / 20.0)
                        }),
                        "down" | "left" => Some(if value <= 0.0011 {
                            0.0
                        } else {
                            value / 10.0f32.powf(step / 20.0)
                        }),
                        "home" => Some(0.0),
                        "end" => Some(2.0),
                        "enter" => Some(1.0),
                        _ => None,
                    };
                    if let Some(value) = next {
                        apply_gain(a, channel, value);
                        w.prevent_default();
                        c.stop_propagation();
                        c.notify();
                    }
                })),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(theme.secondary))
                .child(gain_label(gain)),
        )
}

#[derive(Clone)]
enum Choice {
    Device { input: bool, name: Option<String> },
    Channel(u16),
}
fn choices(app: &VoiceApp, selector: Selector) -> Vec<(String, Choice)> {
    match selector {
        Selector::Input | Selector::Output => {
            let input = selector == Selector::Input;
            let devices = if input {
                &app.devices.inputs
            } else {
                &app.devices.outputs
            };
            let mut items = vec![(
                "System default".into(),
                Choice::Device { input, name: None },
            )];
            items.extend(devices.iter().map(|d| {
                (
                    d.name.clone(),
                    Choice::Device {
                        input,
                        name: Some(d.name.clone()),
                    },
                )
            }));
            items
        }
        Selector::Channel => (0..app.available_input_channels().max(1))
            .map(|channel| (format!("Channel {}", channel + 1), Choice::Channel(channel)))
            .collect(),
    }
}
fn choose(app: &mut VoiceApp, choice: &Choice) {
    match choice {
        Choice::Device { input, name } => app.select_audio_device(*input, name.clone()),
        Choice::Channel(channel) => app.select_input_channel(*channel),
    }
    app.mixer_ui.menu = None;
}
fn selected_index(app: &VoiceApp, selector: Selector) -> usize {
    match selector {
        Selector::Input | Selector::Output => {
            let name = if selector == Selector::Input {
                &app.doc().settings.audio.input_device
            } else {
                &app.doc().settings.audio.output_device
            };
            let devices = if selector == Selector::Input {
                &app.devices.inputs
            } else {
                &app.devices.outputs
            };
            name.as_ref()
                .and_then(|name| devices.iter().position(|d| &d.name == name))
                .map_or(0, |i| i + 1)
        }
        Selector::Channel => app.doc().settings.audio.input_channel as usize,
    }
}
fn open_menu(app: &mut VoiceApp, selector: Selector, bounds: Bounds<Pixels>) {
    let index = selected_index(app, selector).min(choices(app, selector).len().saturating_sub(1));
    app.mixer_ui.drag = None;
    app.mixer_ui.menu_scroll.scroll_to_item(index);
    app.mixer_ui.menu = Some(MenuState {
        selector,
        x: bounds.origin.x.into(),
        y: (bounds.origin.y + bounds.size.height).into(),
        width: bounds.size.width.into(),
        index,
    });
}
fn menu_key(
    app: &mut VoiceApp,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<VoiceApp>,
) -> bool {
    let Some(mut menu) = app.mixer_ui.menu else {
        return false;
    };
    let items = choices(app, menu.selector);
    if items.is_empty() {
        return false;
    }
    match event.keystroke.key.as_str() {
        "up" => menu.index = menu.index.saturating_sub(1),
        "down" => menu.index = (menu.index + 1).min(items.len() - 1),
        "home" => menu.index = 0,
        "end" => menu.index = items.len() - 1,
        "enter" | "space" => {
            choose(app, &items[menu.index.min(items.len() - 1)].1);
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
            return true;
        }
        "escape" => {
            app.mixer_ui.menu = None;
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
            return true;
        }
        _ => return false,
    }
    app.mixer_ui.menu = Some(menu);
    app.mixer_ui.menu_scroll.scroll_to_item(menu.index);
    window.prevent_default();
    cx.stop_propagation();
    cx.notify();
    true
}
fn selector(
    app: &VoiceApp,
    selector: Selector,
    title: &'static str,
    value: String,
    theme: Theme,
    cx: &Context<VoiceApp>,
) -> Div {
    let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
    let paint = bounds.clone();
    let click = bounds.clone();
    let open = app.mixer_ui.menu.is_some_and(|m| m.selector == selector);
    let enabled = app.busy.is_none() && app.panel.is_none();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .child(label(theme, title))
        .child(
            div()
                .relative()
                .w_full()
                .h(px(29.))
                .child(
                    canvas(
                        move |b, _, _| {
                            paint.set(b);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .child(
                    div()
                        .id(match selector {
                            Selector::Input => "mixer-input",
                            Selector::Output => "mixer-output",
                            Selector::Channel => "mixer-channel",
                        })
                        .key_context("VoiceControl")
                        .w_full()
                        .h(px(29.))
                        .px_2()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(if open { theme.accent } else { theme.separator }))
                        .bg(rgb(theme.control))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .when(enabled, |d| d.tab_index(0).cursor_pointer())
                        .when(!enabled, |d| d.opacity(0.42))
                        .focus(move |s| s.border_color(rgb(theme.accent)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .text_size(px(12.))
                                .child(value),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(theme.secondary))
                                .child("⌄"),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |a, _: &MouseDownEvent, _, c| {
                                if !enabled {
                                    return;
                                }
                                if a.mixer_ui.menu.is_some_and(|m| m.selector == selector) {
                                    a.mixer_ui.menu = None;
                                } else {
                                    open_menu(a, selector, click.get());
                                }
                                c.stop_propagation();
                                c.notify();
                            }),
                        )
                        .on_key_down(cx.listener(move |a, event: &KeyDownEvent, w, c| {
                            if !enabled {
                                return;
                            }
                            if menu_key(a, event, w, c) {
                                return;
                            }
                            if matches!(event.keystroke.key.as_str(), "enter" | "space" | "down") {
                                open_menu(a, selector, bounds.get());
                                w.prevent_default();
                                c.stop_propagation();
                                c.notify();
                            }
                        })),
                ),
        )
}

pub(super) fn popup(app: &VoiceApp, window: &Window, cx: &Context<VoiceApp>) -> AnyElement {
    let Some(menu) = app
        .mixer_ui
        .menu
        .filter(|_| app.panel.is_none() && app.sidebar_visible)
    else {
        return div().into_any_element();
    };
    let theme = Theme::for_window(window);
    let items = choices(app, menu.selector);
    let viewport = window.viewport_size();
    let width: f32 = viewport.width.into();
    let height: f32 = viewport.height.into();
    let menu_height = (items.len() as f32 * 29.0 + 8.0).min(250.0);
    let x = menu.x.min(width - menu.width - 8.0).max(8.0);
    let y = (menu.y + 3.0).min(height - menu_height - 8.0).max(8.0);
    let mut list = div()
        .id("mixer-device-menu")
        .w(px(menu.width))
        .max_h(px(menu_height))
        .overflow_y_scroll()
        .track_scroll(&app.mixer_ui.menu_scroll)
        .py_1()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(theme.separator))
        .bg(rgb(theme.content))
        .shadow_lg();
    for (index, (title, choice)) in items.into_iter().enumerate() {
        list = list.child(
            div()
                .id(index)
                .h(px(29.))
                .px_3()
                .flex()
                .items_center()
                .bg(rgb(if index == menu.index {
                    theme.accent
                } else {
                    theme.content
                }))
                .text_color(rgb(if index == menu.index {
                    0xffffff
                } else {
                    theme.text
                }))
                .text_size(px(12.))
                .cursor_pointer()
                .on_hover(cx.listener(move |a, hovered: &bool, _, c| {
                    if *hovered
                        && let Some(menu) = &mut a.mixer_ui.menu
                        && menu.index != index
                    {
                        menu.index = index;
                        c.notify();
                    }
                }))
                .child(title)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |a, _: &MouseDownEvent, w, c| {
                        choose(a, &choice);
                        a.focus.focus(w);
                        c.stop_propagation();
                        c.notify();
                    }),
                ),
        );
    }
    div()
        .absolute()
        .inset_0()
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|a, _, _, c| {
                a.mixer_ui.menu = None;
                c.stop_propagation();
                c.notify();
            }),
        )
        .child(div().absolute().left(px(x)).top(px(y)).child(list))
        .into_any_element()
}

pub(super) fn sidebar(app: &VoiceApp, window: &Window, cx: &Context<VoiceApp>) -> AnyElement {
    let theme = Theme::for_window(window);
    let controls = Controls {
        theme,
        enabled: app.busy.is_none() && app.panel.is_none(),
        tab_stop: app.panel.is_none(),
    };
    let levels = app.mixer_levels();
    let settings = &app.doc().settings;
    let input = settings
        .audio
        .input_device
        .clone()
        .unwrap_or_else(|| "System default".into());
    let output = settings
        .audio
        .output_device
        .clone()
        .unwrap_or_else(|| "System default".into());
    let mut strips = div().flex().justify_between().gap_1();
    for (channel, title, gain, peak, present) in [
        (
            Channel::Voice,
            "Your voice",
            settings.monitor_gain,
            levels[0],
            true,
        ),
        (
            Channel::Reference,
            "Vocal reference",
            settings.target_gain,
            levels[1],
            app.doc().target.is_some(),
        ),
        (
            Channel::Accompaniment,
            "Accompaniment",
            settings.backing_gain,
            levels[2],
            app.doc().backing.is_some(),
        ),
    ] {
        let mut strip = fader(
            app,
            FaderSpec {
                channel,
                title,
                gain,
                peak,
            },
            controls.enabled(present),
            cx,
        );
        strip = strip.child(match channel {
            Channel::Voice => label(theme, "Input level")
                .text_size(px(10.))
                .into_any_element(),
            Channel::Reference => button(
                controls,
                "mute-reference",
                if settings.target_muted {
                    "Muted"
                } else {
                    "Mute"
                },
                settings.target_muted,
                cx,
                |a, _, _| {
                    a.workspace.document.settings.target_muted = !a.doc().settings.target_muted;
                    a.update_mix();
                },
            )
            .into_any_element(),
            Channel::Accompaniment if present => button(
                controls,
                "mute-accompaniment",
                if settings.backing_muted {
                    "Muted"
                } else {
                    "Mute"
                },
                settings.backing_muted,
                cx,
                |a, _, _| {
                    a.workspace.document.settings.backing_muted = !a.doc().settings.backing_muted;
                    a.update_mix();
                },
            )
            .into_any_element(),
            Channel::Accompaniment => button(
                controls,
                "add-accompaniment",
                "Add…",
                false,
                cx,
                |a, w, c| a.choose_import(TrackRole::Backing, w, c),
            )
            .into_any_element(),
        });
        strips = strips.child(strip);
    }
    let active = app.recording_active();
    let pending = app.recording_pending();
    let record_controls = Controls {
        theme: Theme {
            accent: theme.recording,
            ..theme
        },
        ..controls
    };
    let mut content = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(selector(app, Selector::Input, "Input", input, theme, cx))
        .child(selector(app, Selector::Output, "Output", output, theme, cx))
        .child(
            div()
                .flex()
                .items_end()
                .gap_2()
                .child(div().flex_1().child(selector(
                    app,
                    Selector::Channel,
                    "Input channel",
                    format!("Channel {}", settings.audio.input_channel + 1),
                    theme,
                    cx,
                )))
                .child(button(
                    controls,
                    "mixer-advanced",
                    "Advanced…",
                    false,
                    cx,
                    |a, w, c| a.show_audio_settings(w, c),
                )),
        )
        .child(
            label(
                theme,
                app.preferences_status
                    .clone()
                    .unwrap_or_else(|| "Devices are remembered on this Mac.".into()),
            )
            .text_size(px(11.)),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child(
                    button(
                        controls.enabled(!app.permission_pending),
                        "sidebar-mic",
                        if app.permission_pending {
                            "Allow access…"
                        } else if app.input_pending {
                            "Cancel mic"
                        } else if app.input_running {
                            "Mic on"
                        } else {
                            "Mic off"
                        },
                        app.input_running,
                        cx,
                        |a, w, c| a.toggle_input(w, c),
                    )
                    .flex_1(),
                )
                .child(
                    button(
                        controls.enabled(!app.permission_pending),
                        "hear-myself",
                        if settings.audio.monitor {
                            "Hearing myself"
                        } else {
                            "Hear myself"
                        },
                        settings.audio.monitor,
                        cx,
                        |a, w, c| a.toggle_hear_myself(w, c),
                    )
                    .flex_1(),
                ),
        )
        .child(label(theme, "Use headphones when hearing yourself.").text_size(px(10.)))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    button(
                        record_controls.enabled(
                            app.doc().target.is_some()
                                && app.replaying_take.is_none()
                                && !app.recording_finalizing(),
                        ),
                        "record-take",
                        if app.recording_finalizing() {
                            "Saving take…"
                        } else if active {
                            "Stop recording"
                        } else if pending {
                            "Cancel recording"
                        } else {
                            "● Record take"
                        },
                        active,
                        cx,
                        |a, w, c| a.toggle_record(w, c),
                    )
                    .flex_1(),
                )
                .child(button(
                    controls,
                    "sidebar-history",
                    "Takes…",
                    false,
                    cx,
                    |a, w, _| {
                        a.panel = Some(super::Panel::History);
                        a.mixer_ui.dismiss();
                        a.focus.focus(w);
                    },
                )),
        )
        .when(app.recording_status.is_some(), |d| {
            d.child(label(
                theme,
                app.recording_status.clone().unwrap_or_default(),
            ))
        })
        .child(
            div()
                .border_t_1()
                .border_color(rgb(theme.separator))
                .pt_3()
                .child(strips),
        );
    if let Some((index, take)) = app.doc().history.iter().enumerate().next_back() {
        let replay = app.replaying_take.as_deref() == Some(take.id.as_str());
        let same_reference = app
            .doc()
            .target
            .as_ref()
            .is_some_and(|target| target.id == take.target_id);
        let evaluated = take.evaluated_seconds > 0.0;
        content = content.child(
            div()
                .mt_1()
                .pt_3()
                .border_t_1()
                .border_color(rgb(theme.separator))
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(format!(
                            "Latest take · {}",
                            clock_duration(take.end - take.start)
                        )),
                )
                .child(label(
                    theme,
                    if evaluated {
                        format!(
                            "In tune {:.0}% · Out of tune {:.0}%",
                            take.score,
                            (take.coverage - take.score).max(0.)
                        )
                    } else {
                        "No evaluated vocal-reference time".into()
                    },
                ))
                .when(evaluated, |d| {
                    d.child(label(
                        theme,
                        format!(
                            "Missed {:.0}% · Coverage {:.0}%",
                            (100. - take.coverage).max(0.),
                            take.coverage
                        ),
                    ))
                })
                .when(!same_reference, |d| {
                    d.child(label(
                        theme,
                        "Open the original vocal reference to compare this take.",
                    ))
                })
                .child(button(
                    controls.enabled(app.take_audio_available(index) && same_reference),
                    "latest-take-listen",
                    if replay {
                        "Stop listening"
                    } else if app.take_audio_available(index) {
                        "Listen to take"
                    } else {
                        "Pitch trace only"
                    },
                    replay,
                    cx,
                    move |a, _, _| {
                        if replay {
                            a.stop_take_playback();
                        } else {
                            a.play_take(index);
                        }
                    },
                )),
        );
    }
    div()
        .id("persistent-mixer")
        .w(px(330.))
        .flex_shrink_0()
        .h_full()
        .min_h_0()
        .border_l_1()
        .border_color(rgb(theme.separator))
        .bg(rgb(theme.window))
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(38.))
                .px_4()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_between()
                .border_b_1()
                .border_color(rgb(theme.separator))
                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("Mixer"))
                .child(button(
                    controls,
                    "hide-mixer",
                    "Hide",
                    false,
                    cx,
                    |a, _, _| {
                        a.sidebar_visible = false;
                        a.mixer_ui.dismiss();
                    },
                )),
        )
        .child(
            div()
                .id("mixer-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_3()
                .child(content),
        )
        .into_any_element()
}
fn clock_duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    format!(
        "{}:{:02}",
        (seconds / 60.0).floor() as u64,
        (seconds % 60.0).floor() as u64
    )
}
