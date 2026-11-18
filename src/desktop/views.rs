use super::roll::{self, RollData, clock, note_name};
use super::theme::Theme;
use super::*;
use gpui::{
    AnyElement, Div, ExternalPaths, MouseButton, MouseDownEvent, SharedString, canvas, div, rgb,
    rgba,
};
use std::{cell::Cell, rc::Rc};

// The document follows system surfaces. Blue identifies the reference; orange identifies live singing.
// The musical canvas owns the window; controls use the system font, 28px height and a 4px rhythm.
#[derive(Clone, Copy)]
struct Controls {
    theme: Theme,
    enabled: bool,
    tab_stop: bool,
}
impl Controls {
    fn enabled(self, enabled: bool) -> Self {
        Self {
            enabled: self.enabled && enabled,
            ..self
        }
    }
}
fn button(
    controls: Controls,
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    active: bool,
    cx: &Context<VoiceApp>,
    action: impl Fn(&mut VoiceApp, &mut Window, &mut Context<VoiceApp>) + 'static,
) -> impl IntoElement {
    let theme = controls.theme;
    let action = Rc::new(action);
    let keyboard_action = action.clone();
    div()
        .id(id.into())
        .key_context("VoiceControl")
        .flex()
        .items_center()
        .justify_center()
        .min_h(px(28.))
        .px_3()
        .py_1()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(if active {
            theme.accent
        } else {
            theme.separator
        }))
        .bg(rgb(if active { theme.accent } else { theme.control }))
        .text_color(rgb(if active { 0xffffff } else { theme.text }))
        .text_size(px(13.))
        .child(title.into())
        .when(!controls.enabled, |this| this.opacity(0.42))
        .when(controls.enabled, |this| {
            this.cursor_pointer().hover(move |s| {
                s.bg(rgb(if active {
                    theme.accent
                } else {
                    theme.control_hover
                }))
            })
        })
        .when(controls.enabled && controls.tab_stop, |this| {
            this.tab_index(0)
                .focus(move |s| s.border_color(rgb(theme.accent)))
        })
        .on_click(cx.listener(move |app, _, window, cx| {
            if controls.enabled {
                action(app, window, cx);
                cx.notify();
            }
        }))
        .on_key_down(
            cx.listener(move |app, event: &gpui::KeyDownEvent, window, cx| {
                if controls.enabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    window.prevent_default();
                    cx.stop_propagation();
                    keyboard_action(app, window, cx);
                    cx.notify();
                }
            }),
        )
}
fn label(theme: Theme, text: impl Into<SharedString>) -> Div {
    div()
        .text_color(rgb(theme.secondary))
        .text_size(px(12.))
        .child(text.into())
}
fn row(theme: Theme, title: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .min_h(px(30.))
        .child(label(theme, title))
        .child(control)
}
fn section(theme: Theme, title: impl Into<SharedString>) -> Div {
    div()
        .mt_4()
        .mb_1()
        .text_color(rgb(theme.text))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(title.into())
}

impl VoiceApp {
    fn empty(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let controls = Controls {
            theme,
            enabled: self.busy.is_none(),
            tab_stop: self.panel.is_none(),
        };
        div().flex_1().flex().flex_col().items_center().justify_center().gap_4().px_8()
            .child(div().text_size(px(42.)).text_color(rgb(theme.secondary)).child("♪"))
            .child(div().text_size(px(21.)).font_weight(gpui::FontWeight::MEDIUM).child("Add a vocal reference"))
            .child(label(theme,"The recording of the vocal part you want to learn."))
            .child(div().mt_3().flex().gap_3()
                .child(button(controls,"import-first","Import Vocal Reference…",true,cx,|a,w,c|a.choose_import(TrackRole::Target,w,c)))
                .child(button(controls,"open-project","Open Project…",false,cx,|a,w,c|a.choose_open(w,c))))
            .child(label(theme,"or drop a recording into this window"))
            .child(div().mt_5().max_w(px(420.)).text_center().text_size(px(12.)).text_color(rgb(theme.secondary))
                .child("Accompaniment is optional music to sing along with. Add it from the File menu."))
    }
    fn workspace_view(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let controls = Controls {
            theme,
            enabled: self.busy.is_none() && self.panel.is_none(),
            tab_stop: self.panel.is_none(),
        };
        let target_now = scoring::pitch_at(&self.selected.frames, self.cursor);
        let live = self.live.filter(|f| self.input_running && f.voiced());
        let reference_note = target_now.map_or("—".into(), |f| note_name(f.midi));
        let live_note = live.map_or("—".into(), |f| note_name(f.midi));
        let cents = live.and_then(|f| target_now.map(|t| (f.midi - t.midi) * 100.));
        let tuning = cents.map_or("Waiting for your voice".into(), |v| {
            format!("{v:+.0} cents")
        });
        let first = self
            .selected
            .notes
            .partition_point(|n| n.start < self.cursor);
        let interval = if first > 0 && first < self.selected.notes.len() {
            format!(
                "Next: {:+} semitones",
                (self.selected.notes[first].midi - self.selected.notes[first - 1].midi).round()
                    as i32
            )
        } else {
            String::new()
        };
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let paint_bounds = bounds.clone();
        let data = RollData {
            max_note_duration: self.max_note_duration,
            target: self.target.clone(),
            selected: self.selected.clone(),
            trace: Arc::new(self.trace.iter().copied().collect()),
            cursor: self.cursor,
            start: self.view_start,
            seconds: self.doc().transport.visible_seconds,
            loop_range: self.doc().transport.loop_enabled.then_some((
                self.doc().transport.loop_start,
                self.doc().transport.loop_end,
            )),
        };
        let start = data.start;
        let seconds = data.seconds;
        let plot = div()
            .id("pitch-roll")
            .relative()
            .flex_1()
            .min_h(px(200.))
            .overflow_hidden()
            .cursor_pointer()
            .child(
                canvas(
                    move |b, _, _| {
                        paint_bounds.set(b);
                    },
                    move |b, _, w, c| roll::paint_roll(b, &data, w, c),
                )
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |a, e: &MouseDownEvent, _, _| {
                    if a.panel.is_some() {
                        return;
                    }
                    let b = bounds.get();
                    let x: f32 = (e.position.x - b.origin.x).into();
                    let width: f32 = b.size.width.into();
                    if x >= roll::LABEL_WIDTH && width > roll::LABEL_WIDTH {
                        a.seek(
                            start
                                + ((x - roll::LABEL_WIDTH) / (width - roll::LABEL_WIDTH))
                                    .clamp(0., 1.) as f64
                                    * seconds,
                        );
                    }
                }),
            );
        let overview_bounds = Rc::new(Cell::new(Bounds::default()));
        let overview_paint = overview_bounds.clone();
        let target = self.target.clone();
        let cursor = self.cursor;
        let duration = self.doc().duration();
        let view = (
            self.view_start,
            self.view_start + self.doc().transport.visible_seconds,
        );
        let overview = div()
            .id("overview")
            .h(px(38.))
            .flex_shrink_0()
            .overflow_hidden()
            .cursor_pointer()
            .border_t_1()
            .border_color(rgb(theme.separator))
            .child(
                canvas(
                    move |b, _, _| {
                        overview_paint.set(b);
                    },
                    move |b, _, w, _| roll::paint_overview(b, &target, cursor, duration, view, w),
                )
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |a, e: &MouseDownEvent, _, _| {
                    if a.panel.is_some() {
                        return;
                    }
                    let b = overview_bounds.get();
                    let x: f32 = (e.position.x - b.origin.x).into();
                    let width: f32 = b.size.width.into();
                    if width > 0.0 {
                        a.seek((x / width).clamp(0., 1.) as f64 * a.doc().duration());
                    }
                }),
            );
        let toolbar = div()
            .min_h(px(52.))
            .flex_shrink_0()
            .px_4()
            .py_2()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_4()
            .bg(rgb(theme.toolbar))
            .border_b_1()
            .border_color(rgb(theme.separator))
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .child(button(controls, "start", "Start", false, cx, |a, _, _| {
                        a.seek(0.)
                    }))
                    .child(button(
                        controls,
                        "play",
                        if self.play_pending {
                            "Cancel start"
                        } else if self.playing() {
                            "Pause"
                        } else {
                            "Play"
                        },
                        self.playing(),
                        cx,
                        |a, _, _| a.toggle_play(),
                    ))
                    .child(button(
                        controls,
                        "loop",
                        "Loop",
                        self.doc().transport.loop_enabled,
                        cx,
                        |a, _, _| {
                            a.workspace.document.transport.loop_enabled =
                                !a.doc().transport.loop_enabled;
                            a.update_loop();
                        },
                    ))
                    .child(div().w(px(158.)).ml_2().text_size(px(13.)).child(format!(
                        "{}  /  {}",
                        clock(self.cursor),
                        clock(self.doc().duration())
                    ))),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .child(button(
                        controls,
                        "set-a",
                        format!("A {}", clock(self.doc().transport.loop_start)),
                        false,
                        cx,
                        |a, _, _| {
                            a.workspace.document.transport.loop_start =
                                a.cursor.min((a.doc().duration() - 0.1).max(0.));
                            if a.doc().transport.loop_end <= a.doc().transport.loop_start {
                                a.workspace.document.transport.loop_end =
                                    (a.cursor + 5.).min(a.doc().duration());
                            }
                            a.update_loop();
                        },
                    ))
                    .child(button(
                        controls,
                        "set-b",
                        format!("B {}", clock(self.doc().transport.loop_end)),
                        false,
                        cx,
                        |a, _, _| {
                            if a.cursor > a.doc().transport.loop_start + 0.1 {
                                a.workspace.document.transport.loop_end = a.cursor;
                                a.update_loop();
                            }
                        },
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .child(button(
                        controls.enabled(self.engine.is_some() && !self.permission_pending),
                        "mic",
                        if self.permission_pending {
                            "Requesting access…"
                        } else if self.input_pending {
                            "Cancel microphone"
                        } else if self.input_running {
                            "Microphone on"
                        } else {
                            "Microphone off"
                        },
                        self.input_running,
                        cx,
                        |a, w, c| a.toggle_input(w, c),
                    ))
                    .child(button(controls, "mix", "Mix…", false, cx, |a, w, _| {
                        a.panel = Some(Panel::Mix);
                        a.focus.focus(w);
                    }))
                    .child(button(
                        controls,
                        "reference-review",
                        if self.target.mode == AnalysisMode::Polyphonic && !self.target.reviewed {
                            "Review reference…"
                        } else {
                            "Reference…"
                        },
                        false,
                        cx,
                        |a, w, _| {
                            a.panel = Some(Panel::Target);
                            a.focus.focus(w);
                        },
                    )),
            );
        let legend = div()
            .h(px(34.))
            .px_4()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(rgb(theme.separator))
            .bg(rgb(theme.content))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(12.))
                                    .h(px(3.))
                                    .rounded(px(2.))
                                    .bg(rgb(theme.accent)),
                            )
                            .child("Vocal reference"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(12.))
                                    .h(px(3.))
                                    .rounded(px(2.))
                                    .bg(rgb(theme.live)),
                            )
                            .child("Your voice"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(label(theme, interval))
                    .child(button(controls, "zoom-in", "−", false, cx, |a, _, _| {
                        a.workspace.document.transport.visible_seconds =
                            (a.doc().transport.visible_seconds / 1.5).max(3.);
                        a.changed();
                    }))
                    .child(label(
                        theme,
                        format!("{:.0} s", self.doc().transport.visible_seconds),
                    ))
                    .child(button(controls, "zoom-out", "+", false, cx, |a, _, _| {
                        a.workspace.document.transport.visible_seconds =
                            (a.doc().transport.visible_seconds * 1.5).min(120.);
                        a.changed();
                    })),
            );
        let score = if !self.score_ready() {
            "Review the reference line before scoring".into()
        } else if self.input_running && self.score.target_seconds > 0.0 {
            format!(
                "{:.0}% in tune · {:.0}% coverage",
                self.score.score, self.score.coverage
            )
        } else if self.permission_pending {
            "Waiting for microphone access…".into()
        } else if self.input_pending {
            "Connecting microphone…".into()
        } else if self.input_running {
            "Listening".into()
        } else {
            "Microphone off".into()
        };
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(theme.content))
            .child(toolbar)
            .child(legend)
            .child(plot)
            .child(overview)
            .child(
                div()
                    .h(px(42.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .border_t_1()
                    .border_color(rgb(theme.separator))
                    .bg(rgb(theme.toolbar))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_5()
                            .child(
                                div()
                                    .text_color(rgb(theme.accent))
                                    .child(format!("Reference {reference_note}")),
                            )
                            .child(
                                div()
                                    .text_color(rgb(theme.live))
                                    .child(format!("Your voice {live_note}")),
                            )
                            .child(label(theme, tuning)),
                    )
                    .child(label(theme, score)),
            )
    }
    fn panel_content(&self, panel: Panel, window: &Window, cx: &Context<Self>) -> Div {
        let theme = Theme::for_window(window);
        let controls = Controls {
            theme,
            enabled: self.busy.is_none(),
            tab_stop: true,
        };
        let mut content = div().flex().flex_col().gap_3();
        match panel {
            Panel::Mix => {
                for role in [TrackRole::Target, TrackRole::Backing] {
                    let track = self.doc().track(role);
                    let present = track.is_some();
                    let name = track.map_or(
                        if role == TrackRole::Backing {
                            "Not added · optional"
                        } else {
                            "No vocal reference"
                        }
                        .to_string(),
                        |t| t.filename.clone(),
                    );
                    let (gain, muted) = if role == TrackRole::Target {
                        (
                            self.doc().settings.target_gain,
                            self.doc().settings.target_muted,
                        )
                    } else {
                        (
                            self.doc().settings.backing_gain,
                            self.doc().settings.backing_muted,
                        )
                    };
                    let prefix = if role == TrackRole::Target {
                        "Vocal reference"
                    } else {
                        "Accompaniment"
                    };
                    content = content
                        .child(section(theme, prefix))
                        .child(div().text_size(px(12.)).overflow_hidden().child(name))
                        .child(button(
                            controls,
                            format!("import-{prefix}"),
                            if present {
                                "Replace audio…"
                            } else {
                                "Choose audio…"
                            },
                            false,
                            cx,
                            move |a, w, c| a.choose_import(role, w, c),
                        ));
                    if present {
                        content = content
                            .child(row(
                                theme,
                                "Level",
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(button(
                                        controls,
                                        format!("down-{prefix}"),
                                        "-",
                                        false,
                                        cx,
                                        move |a, _, _| {
                                            let s = &mut a.workspace.document.settings;
                                            if role == TrackRole::Target {
                                                s.target_gain = (s.target_gain - 0.05).max(0.);
                                            } else {
                                                s.backing_gain = (s.backing_gain - 0.05).max(0.);
                                            }
                                            a.update_mix();
                                        },
                                    ))
                                    .child(format!("{:.0}%", gain * 100.))
                                    .child(button(
                                        controls,
                                        format!("up-{prefix}"),
                                        "+",
                                        false,
                                        cx,
                                        move |a, _, _| {
                                            let s = &mut a.workspace.document.settings;
                                            if role == TrackRole::Target {
                                                s.target_gain = (s.target_gain + 0.05).min(1.5);
                                            } else {
                                                s.backing_gain = (s.backing_gain + 0.05).min(1.5);
                                            }
                                            a.update_mix();
                                        },
                                    )),
                            ))
                            .child(button(
                                controls,
                                format!("mute-{prefix}"),
                                if muted { "Muted" } else { "Mute" },
                                muted,
                                cx,
                                move |a, _, _| {
                                    let s = &mut a.workspace.document.settings;
                                    if role == TrackRole::Target {
                                        s.target_muted = !s.target_muted;
                                    } else {
                                        s.backing_muted = !s.backing_muted;
                                    }
                                    a.update_mix();
                                },
                            ));
                        if role == TrackRole::Backing {
                            content = content.child(button(
                                controls,
                                "remove-backing",
                                "Remove accompaniment",
                                false,
                                cx,
                                |a, _, _| {
                                    a.pause();
                                    a.workspace.document.backing = None;
                                    a.workspace.media_paths.remove(&TrackRole::Backing);
                                    a.changed();
                                    if let Err(e) = a.sync_audio() {
                                        a.fail(e);
                                    }
                                },
                            ));
                        }
                    }
                }
            }
            Panel::Target => {
                content = content
                    .child(label(theme, "The recording contains"))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(button(
                                controls,
                                "mono",
                                "One voice",
                                self.mode == AnalysisMode::Monophonic,
                                cx,
                                |a, _, _| a.mode = AnalysisMode::Monophonic,
                            ))
                            .child(button(
                                controls,
                                "poly",
                                "Several voices or notes",
                                self.mode == AnalysisMode::Polyphonic,
                                cx,
                                |a, _, _| a.mode = AnalysisMode::Polyphonic,
                            )),
                    )
                    .child(button(
                        controls,
                        "replace-target",
                        "Choose vocal reference…",
                        false,
                        cx,
                        |a, w, c| a.choose_import(TrackRole::Target, w, c),
                    ));
                if self.doc().target.is_some() {
                    content = content
                        .child(button(
                            controls,
                            "reanalyze",
                            "Analyze again",
                            false,
                            cx,
                            |a, _, _| {
                                if let Some(p) =
                                    a.workspace.media_paths.get(&TrackRole::Target).cloned()
                                {
                                    a.import_path(TrackRole::Target, p);
                                }
                            },
                        ))
                        .child(label(theme, format!("{} notes", self.target.notes.len())));
                }
                if self.target.mode == AnalysisMode::Polyphonic {
                    content=content.child(section(theme, "Choose the part to learn")).child(label(theme, "Gray notes are detected candidates. Blue is the part you will practise. Listen before confirming."))
                        .child(div().flex().gap_2().child(button(controls, "highest","Higher notes",self.target.selection.policy==LinePolicy::Highest,cx,|a,_,_|a.change_selection(|s|s.policy=LinePolicy::Highest))).child(button(controls, "lowest","Lower notes",self.target.selection.policy==LinePolicy::Lowest,cx,|a,_,_|a.change_selection(|s|s.policy=LinePolicy::Lowest))));
                    for lower in [true, false] {
                        let midi = if lower {
                            self.target.selection.min_midi
                        } else {
                            self.target.selection.max_midi
                        };
                        content = content.child(row(
                            theme,
                            if lower { "Lowest note" } else { "Highest note" },
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(button(
                                    controls,
                                    if lower {
                                        "range-min-down"
                                    } else {
                                        "range-max-down"
                                    },
                                    "-",
                                    false,
                                    cx,
                                    move |a, _, _| {
                                        a.change_selection(|s| {
                                            if lower {
                                                s.min_midi = s.min_midi.saturating_sub(1).max(36);
                                            } else {
                                                s.max_midi =
                                                    s.max_midi.saturating_sub(1).max(s.min_midi);
                                            }
                                        })
                                    },
                                ))
                                .child(note_name(midi as f32))
                                .child(button(
                                    controls,
                                    if lower {
                                        "range-min-up"
                                    } else {
                                        "range-max-up"
                                    },
                                    "+",
                                    false,
                                    cx,
                                    move |a, _, _| {
                                        a.change_selection(|s| {
                                            if lower {
                                                s.min_midi = (s.min_midi + 1).min(s.max_midi);
                                            } else {
                                                s.max_midi = (s.max_midi + 1).min(86);
                                            }
                                        })
                                    },
                                )),
                        ));
                    }
                    content = content.child(button(controls, "review", if self.target.reviewed { "Reference line reviewed" } else { "Use this reference line" }, self.target.reviewed, cx, |app,_,_|app.confirm_target()))
                        .child(label(theme, "Note detection is an estimate. It does not identify individual singers or choir parts."));
                }
                content = content.child(section(theme, "Scoring")).child(row(
                    theme,
                    "Tolerance",
                    div()
                        .flex()
                        .gap_2()
                        .items_center()
                        .child(button(
                            controls,
                            "tolerance-down",
                            "-",
                            false,
                            cx,
                            |a, _, _| {
                                a.pause();
                                a.workspace.document.settings.tolerance_cents =
                                    (a.doc().settings.tolerance_cents - 5.).max(10.);
                                a.changed();
                            },
                        ))
                        .child(format!("{:.0} cents", self.doc().settings.tolerance_cents))
                        .child(button(
                            controls,
                            "tolerance-up",
                            "+",
                            false,
                            cx,
                            |a, _, _| {
                                a.pause();
                                a.workspace.document.settings.tolerance_cents =
                                    (a.doc().settings.tolerance_cents + 5.).min(100.);
                                a.changed();
                            },
                        )),
                ));
            }
            Panel::Audio => {
                let permission = match self.microphone_authorization {
                    platform::MicrophoneAuthorization::Authorized => {
                        "Microphone access is allowed."
                    }
                    platform::MicrophoneAuthorization::NotDetermined => {
                        "Allow microphone access to see your singing in real time."
                    }
                    platform::MicrophoneAuthorization::Denied => {
                        "Microphone access is turned off in System Settings."
                    }
                    platform::MicrophoneAuthorization::Restricted => {
                        "Microphone access is restricted on this Mac."
                    }
                };
                content = content
                    .child(section(theme, "Microphone access"))
                    .child(label(
                        theme,
                        if self.permission_pending {
                            "Waiting for microphone permission…"
                        } else {
                            permission
                        },
                    ));
                if matches!(
                    self.microphone_authorization,
                    platform::MicrophoneAuthorization::Denied
                        | platform::MicrophoneAuthorization::Restricted
                ) {
                    content = content.child(button(
                        controls,
                        "microphone-settings",
                        "Open System Settings…",
                        false,
                        cx,
                        |a, _, c| a.open_microphone_settings(c),
                    ));
                } else {
                    content = content.child(button(
                        controls.enabled(!self.permission_pending),
                        "microphone-access",
                        if self.input_pending {
                            "Cancel microphone connection"
                        } else if self.input_running {
                            "Turn off microphone"
                        } else if self.microphone_authorization
                            == platform::MicrophoneAuthorization::NotDetermined
                        {
                            "Allow microphone access…"
                        } else {
                            "Turn on microphone"
                        },
                        self.input_running,
                        cx,
                        |a, w, c| a.toggle_input(w, c),
                    ));
                }
                content = content
                    .child(label(
                        theme,
                        if self.audio_unresponsive {
                            "Audio device is not responding."
                        } else if self.audio_starting {
                            "Starting audio..."
                        } else if self.audio_configuring {
                            "Applying audio settings..."
                        } else {
                            "Choose devices, then apply settings."
                        },
                    ))
                    .child(button(
                        controls,
                        "refresh-devices",
                        "Refresh devices",
                        false,
                        cx,
                        |a, _, _| {
                            if let Some(e) = &a.engine
                                && let Err(error) = e.refresh_devices()
                            {
                                a.fail(error);
                            }
                        },
                    ))
                    .child(section(theme, "Input device"));
                let input_selected = &self.doc().settings.audio.input_device;
                content = content.child(button(
                    controls,
                    "input-default",
                    "System default",
                    input_selected.is_none(),
                    cx,
                    |a, _, _| {
                        a.workspace.document.settings.audio.input_device = None;
                        a.changed();
                    },
                ));
                for (i, d) in self.devices.inputs.iter().enumerate() {
                    let name = d.name.clone();
                    content = content.child(button(
                        controls,
                        format!("input-{i}"),
                        if d.channels == 0 {
                            d.name.clone()
                        } else {
                            format!("{}  /  {} ch", d.name, d.channels)
                        },
                        input_selected.as_ref() == Some(&name),
                        cx,
                        move |a, _, _| {
                            a.workspace.document.settings.audio.input_device = Some(name.clone());
                            a.changed();
                        },
                    ));
                }
                let channel = self.doc().settings.audio.input_channel;
                content = content
                    .child(row(
                        theme,
                        "Input channel",
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(button(
                                controls,
                                "channel-down",
                                "-",
                                false,
                                cx,
                                |a, _, _| {
                                    a.workspace.document.settings.audio.input_channel =
                                        a.doc().settings.audio.input_channel.saturating_sub(1);
                                    a.changed();
                                },
                            ))
                            .child(format!("{}", channel + 1))
                            .child(button(controls, "channel-up", "+", false, cx, |a, _, _| {
                                a.workspace.document.settings.audio.input_channel =
                                    (a.doc().settings.audio.input_channel + 1).min(63);
                                a.changed();
                            })),
                    ))
                    .child(row(
                        theme,
                        "Input gain",
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(button(
                                controls,
                                "input-gain-down",
                                "-",
                                false,
                                cx,
                                |a, _, _| {
                                    a.workspace.document.settings.audio.input_gain =
                                        (a.doc().settings.audio.input_gain - 0.1).max(0.);
                                    a.changed();
                                },
                            ))
                            .child(format!("{:.1}x", self.doc().settings.audio.input_gain))
                            .child(button(
                                controls,
                                "input-gain-up",
                                "+",
                                false,
                                cx,
                                |a, _, _| {
                                    a.workspace.document.settings.audio.input_gain =
                                        (a.doc().settings.audio.input_gain + 0.1).min(4.);
                                    a.changed();
                                },
                            )),
                    ))
                    .child(section(theme, "Output device"));
                let output_selected = &self.doc().settings.audio.output_device;
                content = content.child(button(
                    controls,
                    "output-default",
                    "System default",
                    output_selected.is_none(),
                    cx,
                    |a, _, _| {
                        a.workspace.document.settings.audio.output_device = None;
                        a.changed();
                    },
                ));
                for (i, d) in self.devices.outputs.iter().enumerate() {
                    let name = d.name.clone();
                    content = content.child(button(
                        controls,
                        format!("output-{i}"),
                        d.name.clone(),
                        output_selected.as_ref() == Some(&name),
                        cx,
                        move |a, _, _| {
                            a.workspace.document.settings.audio.output_device = Some(name.clone());
                            a.changed();
                        },
                    ));
                }
                let mut buffers = div().flex().flex_wrap().gap_2();
                for frames in [64, 128, 256, 512, 1024] {
                    buffers = buffers.child(button(
                        controls,
                        format!("buffer-{frames}"),
                        frames.to_string(),
                        self.doc().settings.audio.buffer_frames == frames,
                        cx,
                        move |a, _, _| {
                            a.workspace.document.settings.audio.buffer_frames = frames;
                            a.changed();
                        },
                    ));
                }
                content = content
                    .child(section(theme, "Buffer frames"))
                    .child(buffers)
                    .child(button(
                        controls,
                        "apply-audio",
                        "Apply audio settings",
                        true,
                        cx,
                        |a, _, _| a.apply_audio(),
                    ))
                    .child(button(
                        controls,
                        "monitor",
                        if self.doc().settings.audio.monitor {
                            "Monitoring enabled"
                        } else {
                            "Enable monitoring"
                        },
                        self.doc().settings.audio.monitor,
                        cx,
                        |a, _, _| {
                            if !a.input_running {
                                a.fail("Turn on the microphone before enabling monitoring.");
                                return;
                            }
                            a.workspace.document.settings.audio.monitor =
                                !a.doc().settings.audio.monitor;
                            if let Some(e) = &a.engine {
                                e.set_monitor(a.doc().settings.audio.monitor);
                            }
                            a.changed();
                        },
                    ))
                    .child(label(theme, "Use headphones when monitoring."));
                let compensation = self.doc().settings.audio.latency_ms;
                content = content
                    .child(row(
                        theme,
                        "Timing correction",
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(button(
                                controls,
                                "latency-down",
                                "-",
                                false,
                                cx,
                                |a, _, _| {
                                    a.workspace.document.settings.audio.latency_ms =
                                        (a.doc().settings.audio.latency_ms - 5.).max(-500.);
                                    a.changed();
                                },
                            ))
                            .child(format!("{compensation:.0} ms"))
                            .child(button(controls, "latency-up", "+", false, cx, |a, _, _| {
                                a.workspace.document.settings.audio.latency_ms =
                                    (a.doc().settings.audio.latency_ms + 5.).min(500.);
                                a.changed();
                            })),
                    ))
                    .child(label(theme, "Positive values move captured pitch earlier."));
                if let Some(e) = &self.engine {
                    let d = e.diagnostics();
                    content = content
                        .child(section(theme, "Active stream"))
                        .child(label(
                            theme,
                            format!(
                                "{} Hz  /  {} output channels",
                                d.sample_rate, d.output_channels
                            ),
                        ))
                        .child(label(
                            theme,
                            format!(
                                "Actual buffer: {} out / {} in",
                                d.output_buffer_frames, d.input_buffer_frames
                            ),
                        ))
                        .child(label(
                            theme,
                            format!("{} underruns / {} input drops", d.underruns, d.input_drops),
                        ));
                    if d.buffer_fallback {
                        content =
                            content.child(label(theme, "The device chose a supported buffer."));
                    }
                    if d.stream_error {
                        content = content.child(
                            div()
                                .text_color(rgb(theme.warning))
                                .child("Audio stream interrupted. Apply settings to reconnect."),
                        );
                    }
                }
            }
            Panel::History => {
                if self.doc().history.is_empty() {
                    content = content.child(label(theme,
                        "No practice takes yet. Turn on the microphone and sing along with the vocal reference.",
                    ));
                }
                for (i, take) in self.doc().history.iter().enumerate().rev() {
                    let start = take.start;
                    content = content
                        .child(button(
                            controls,
                            format!("take-{i}"),
                            format!(
                                "Take {} · {:.0}% in tune · {}",
                                i + 1,
                                take.score,
                                clock(start)
                            ),
                            false,
                            cx,
                            move |a, _, _| a.seek(start),
                        ))
                        .child(label(
                            theme,
                            format!(
                                "{:.0}% coverage · {:.1} seconds evaluated",
                                take.coverage, take.evaluated_seconds
                            ),
                        ));
                }
            }
            Panel::Help => {
                for (key, description) in [
                    ("Space", "Play / pause"),
                    ("Enter", "Return to start"),
                    ("Left / Right", "Seek one second"),
                    ("L", "Toggle loop"),
                    ("Cmd+I", "Choose vocal reference"),
                    ("Cmd+O", "Open project"),
                    ("Cmd+S", "Save project"),
                    ("Cmd+Shift+S", "Save as"),
                    ("Cmd+N", "New project"),
                    ("Escape", "Close sheet"),
                    ("Cmd+1-4", "Mix / Reference / Audio / History"),
                    ("Cmd+/", "Keyboard shortcuts"),
                ] {
                    content = content.child(row(theme, key, div().child(description)));
                }
            }
        }
        content
    }
    pub(super) fn audio_settings_view(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        self.panel_content(Panel::Audio, window, cx)
            .tab_group()
            .key_context("VoiceSheet")
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, w, c| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        w.focus_prev();
                    } else {
                        w.focus_next();
                    }
                    w.prevent_default();
                    c.stop_propagation();
                }
            }))
            .into_any_element()
    }
    fn panel_view(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let Some(panel) = self.panel else {
            return div().into_any_element();
        };
        let theme = Theme::for_window(window);
        let controls = Controls {
            theme,
            enabled: self.busy.is_none(),
            tab_stop: true,
        };
        let title = match panel {
            Panel::Mix => "Mix",
            Panel::Audio => "Audio Settings",
            Panel::Target => "Vocal reference",
            Panel::History => "Practice history",
            Panel::Help => "Keyboard shortcuts",
        };
        let mut content = self.panel_content(panel, window, cx);
        let viewport_height: f32 = window.viewport_size().height.into();
        let max_height = (viewport_height - 100.0).max(220.0);
        if matches!(panel, Panel::Mix | Panel::Target) && self.doc().target.is_some() {
            content = content.child(section(theme, "Listen")).child(button(
                controls,
                "sheet-listen",
                if self.playing() {
                    "Pause playback"
                } else {
                    "Play reference"
                },
                self.playing(),
                cx,
                |a, _, _| a.toggle_play(),
            ));
        }
        if let Some(busy) = &self.busy {
            let progress = self.progress.load(Ordering::Relaxed) as f32 / 10.0;
            content = content
                .child(label(theme, format!("{busy}… {progress:.0}%")))
                .child(button(
                    Controls {
                        enabled: true,
                        ..controls
                    },
                    "sheet-cancel",
                    "Cancel analysis",
                    false,
                    cx,
                    |a, _, _| a.cancel.store(true, Ordering::Relaxed),
                ));
        }
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x00000020))
            .flex()
            .items_center()
            .justify_center()
            .p_8()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|a, _, w, c| {
                    a.panel = None;
                    a.focus.focus(w);
                    c.notify();
                }),
            )
            .child(
                div()
                    .id("document-sheet")
                    .key_context("VoiceSheet")
                    .tab_group()
                    .w(px(600.))
                    .max_w_full()
                    .max_h(px(max_height))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(rgb(theme.separator))
                    .bg(rgb(theme.window))
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px_5()
                            .py_4()
                            .flex_shrink_0()
                            .border_b_1()
                            .border_color(rgb(theme.separator))
                            .child(
                                div()
                                    .text_size(px(17.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .id("sheet-scroll")
                            .flex_1()
                            .min_h_0()
                            .max_h(px(max_height - 112.0))
                            .overflow_y_scroll()
                            .p_5()
                            .child(content),
                    )
                    .child(
                        div()
                            .px_5()
                            .py_3()
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .border_t_1()
                            .border_color(rgb(theme.separator))
                            .child(button(
                                Controls {
                                    enabled: true,
                                    ..controls
                                },
                                "close-sheet",
                                "Done",
                                true,
                                cx,
                                |a, w, _| {
                                    a.panel = None;
                                    a.focus.focus(w);
                                },
                            )),
                    ),
            )
            .into_any_element()
    }
}
impl Render for VoiceApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let body = if self.doc().target.is_some() {
            self.workspace_view(window, cx).into_any_element()
        } else {
            self.empty(window, cx).into_any_element()
        };
        let controls = Controls {
            theme,
            enabled: true,
            tab_stop: self.panel.is_none(),
        };
        let mut root = div()
            .id("voice-root")
            .relative()
            .size_full()
            .tab_group()
            .key_context("Voice")
            .track_focus(&self.focus)
            .font_family(".SystemUIFont")
            .text_size(px(13.))
            .text_color(rgb(theme.text))
            .bg(rgb(theme.window))
            .flex()
            .flex_col()
            .on_drop(cx.listener(|a, paths: &ExternalPaths, w, c| {
                if a.panel.is_some() {
                    return;
                }
                if let Some(path) = paths.paths().first() {
                    if path.extension().is_some_and(|e| e == "voice") {
                        let path = path.clone();
                        a.request_document_action(DocumentIntent::Open(path), w, c);
                    } else {
                        a.import_drop(paths.paths(), w, c);
                    }
                }
            }))
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, w, c| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        w.focus_prev();
                    } else {
                        w.focus_next();
                    }
                    w.prevent_default();
                    c.stop_propagation();
                }
            }))
            .on_action(cx.listener(|a, _: &ShowMix, w, c| {
                a.panel = if a.panel == Some(Panel::Mix) {
                    None
                } else {
                    Some(Panel::Mix)
                };
                a.focus.focus(w);
                c.notify();
            }))
            .on_action(cx.listener(|a, _: &ShowTarget, w, c| {
                a.panel = if a.panel == Some(Panel::Target) {
                    None
                } else {
                    Some(Panel::Target)
                };
                a.focus.focus(w);
                c.notify();
            }))
            .on_action(cx.listener(|a, _: &ShowAudio, w, c| a.show_audio_settings(w, c)))
            .on_action(cx.listener(|a, _: &ShowHistory, w, c| {
                a.panel = if a.panel == Some(Panel::History) {
                    None
                } else {
                    Some(Panel::History)
                };
                a.focus.focus(w);
                c.notify();
            }))
            .on_action(cx.listener(|a, _: &ShowHelp, w, c| {
                a.panel = if a.panel == Some(Panel::Help) {
                    None
                } else {
                    Some(Panel::Help)
                };
                a.focus.focus(w);
                c.notify();
            }))
            .on_action(cx.listener(|a, _: &TogglePlayback, _, _| {
                if a.panel.is_none() {
                    a.toggle_play();
                }
            }))
            .on_action(cx.listener(|a, _: &ReturnToStart, _, _| {
                if a.panel.is_none() {
                    a.seek(0.);
                }
            }))
            .on_action(cx.listener(|a, _: &OpenProject, w, c| a.choose_open(w, c)))
            .on_action(cx.listener(|a, _: &SaveProject, w, c| a.choose_save(false, w, c)))
            .on_action(cx.listener(|a, _: &SaveProjectAs, w, c| a.choose_save(true, w, c)))
            .on_action(cx.listener(|a, _: &NewProject, w, c| a.new_project(w, c)))
            .on_action(
                cx.listener(|a, _: &ImportTarget, w, c| a.choose_import(TrackRole::Target, w, c)),
            )
            .on_action(cx.listener(|a, _: &ImportAccompaniment, w, c| {
                a.choose_import(TrackRole::Backing, w, c)
            }))
            .on_action(cx.listener(|a, _: &ToggleMicrophone, w, c| a.toggle_input(w, c)))
            .on_action(cx.listener(|a, _: &ToggleMonitoring, _, _| a.toggle_monitoring()))
            .on_action(cx.listener(|a, _: &CloseDocument, w, c| a.close_document(w, c)))
            .on_action(cx.listener(|a, _: &Quit, w, c| a.quit(w, c)))
            .on_action(cx.listener(|a, _: &SeekBack, _, _| {
                if a.panel.is_none() {
                    a.seek(a.cursor - 1.);
                }
            }))
            .on_action(cx.listener(|a, _: &SeekForward, _, _| {
                if a.panel.is_none() {
                    a.seek(a.cursor + 1.);
                }
            }))
            .on_action(cx.listener(|a, _: &ToggleLoop, _, _| {
                if a.panel.is_none() {
                    a.workspace.document.transport.loop_enabled = !a.doc().transport.loop_enabled;
                    a.update_loop();
                }
            }))
            .on_action(cx.listener(|a, _: &ClosePanel, w, _| {
                a.panel = None;
                a.focus.focus(w);
            }))
            .child(body);
        if let Some(busy) = &self.busy {
            let progress = self.progress.load(Ordering::Relaxed) as f32 / 10.0;
            root = root.child(
                div()
                    .h(px(36.))
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .border_t_1()
                    .border_color(rgb(theme.separator))
                    .child(label(theme, format!("{busy}… {progress:.0}%")))
                    .when(busy.starts_with("Analyzing"), |this| {
                        this.child(button(
                            controls,
                            "cancel-job",
                            "Cancel",
                            false,
                            cx,
                            |a, _, _| a.cancel.store(true, Ordering::Relaxed),
                        ))
                    }),
            );
        }
        if let Some(notice) = &self.notice {
            root = root.child(
                div()
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .border_t_1()
                    .border_color(rgb(theme.separator))
                    .text_color(rgb(if self.error {
                        theme.warning
                    } else {
                        theme.secondary
                    }))
                    .text_size(px(12.))
                    .child(div().flex_1().child(notice.clone()))
                    .child(button(
                        controls,
                        "dismiss",
                        "Dismiss",
                        false,
                        cx,
                        |a, _, _| a.notice = None,
                    )),
            );
        }
        root.child(self.panel_view(window, cx))
    }
}
