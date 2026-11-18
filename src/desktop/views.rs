use super::roll::{self, AMBER, EDGE, MINT, MUTED, RollData, clock, note_name};
use super::*;
use gpui::{
    AnyElement, Div, ExternalPaths, MouseButton, MouseDownEvent, SharedString, canvas, div, rgb,
};
use std::{cell::Cell, rc::Rc};

fn button(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    active: bool,
    cx: &Context<VoiceApp>,
    action: impl Fn(&mut VoiceApp, &mut Window, &mut Context<VoiceApp>) + 'static,
) -> impl IntoElement {
    div()
        .id(id.into())
        .flex()
        .items_center()
        .justify_center()
        .px_3()
        .py_2()
        .rounded(px(4.))
        .border_1()
        .border_color(rgb(if active { 0x799f87 } else { EDGE }))
        .bg(rgb(if active { 0x293b2f } else { 0x1c241f }))
        .text_color(rgb(if active { MINT } else { 0xc8d0c9 }))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(0x303d33)).border_color(rgb(0x58705f)))
        .child(label.into())
        .on_click(cx.listener(move |app, _, window, cx| {
            action(app, window, cx);
            cx.notify();
        }))
}
fn label(text: impl Into<SharedString>) -> Div {
    div().text_color(rgb(MUTED)).child(text.into())
}
fn row(title: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .child(label(title))
        .child(control)
}
fn section(title: &'static str) -> Div {
    div().mt_4().mb_2().text_color(rgb(MINT)).child(title)
}

impl VoiceApp {
    fn header(&self, cx: &Context<Self>) -> impl IntoElement {
        let title = format!("{}{}", self.doc().title, if self.dirty { " *" } else { "" });
        div()
            .h(px(64.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .px_6()
            .border_b_1()
            .border_color(rgb(EDGE))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(div().text_size(px(15.)).child(title))
                    .child(label(self.workspace.file_path.as_ref().map_or(
                        "Unsaved project".to_string(),
                        |p| {
                            p.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned()
                        },
                    ))),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(button("new", "New", false, cx, |a, w, c| {
                        a.new_project(w, c)
                    }))
                    .child(button("open", "Open", false, cx, |a, w, c| {
                        a.choose_open(w, c)
                    }))
                    .child(button("save", "Save", self.dirty, cx, |a, _, c| {
                        a.choose_save(false, c)
                    })),
            )
    }
    fn empty(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .justify_center()
            .items_center()
            .gap_6()
            .child(
                div()
                    .text_size(px(26.))
                    .text_color(rgb(0xe7eade))
                    .child("Add a target recording"),
            )
            .child(label("One isolated voice, or several notes at once."))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(button(
                        "empty-mono",
                        "One voice",
                        self.mode == AnalysisMode::Monophonic,
                        cx,
                        |a, _, _| a.mode = AnalysisMode::Monophonic,
                    ))
                    .child(button(
                        "empty-poly",
                        "Polyphonic",
                        self.mode == AnalysisMode::Polyphonic,
                        cx,
                        |a, _, _| a.mode = AnalysisMode::Polyphonic,
                    )),
            )
            .child(button(
                "import-first",
                "Import target...",
                true,
                cx,
                |a, _, c| a.choose_import(TrackRole::Target, c),
            ))
            .child(label("Drop an audio file here. Backing track is optional."))
            .child(
                div()
                    .mt_6()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child("WAV  /  FLAC  /  MP3  /  M4A  /  OGG"),
            )
    }
    fn workspace_view(&self, cx: &Context<Self>) -> impl IntoElement {
        let time = format!(
            "{}  /  {}",
            clock(self.cursor),
            clock(self.doc().duration())
        );
        let live_label = self
            .live
            .filter(|f| f.voiced())
            .map_or("--".into(), |f| note_name(f.midi));
        let target_now = scoring::pitch_at(&self.selected.frames, self.cursor);
        let target_label = target_now.map_or("--".into(), |f| note_name(f.midi));
        let cents = self
            .live
            .filter(|f| f.voiced())
            .and_then(|f| target_now.map(|t| (f.midi - t.midi) * 100.));
        let tuning = cents.map_or("-- cents".into(), |v| format!("{v:+.0} cents"));
        let first = self
            .selected
            .notes
            .partition_point(|n| n.start < self.cursor);
        let interval = if first > 0 && first < self.selected.notes.len() {
            let v = (self.selected.notes[first].midi - self.selected.notes[first - 1].midi).round()
                as i32;
            format!("Next {v:+} st")
        } else {
            "Next --".into()
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
        let overview_bounds = Rc::new(Cell::new(Bounds::default()));
        let overview_paint = overview_bounds.clone();
        let target = self.target.clone();
        let cursor = self.cursor;
        let duration = self.doc().duration();
        let view = (
            self.view_start,
            self.view_start + self.doc().transport.visible_seconds,
        );
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
                    let b = bounds.get();
                    let x: f32 = (e.position.x - b.origin.x).into();
                    let width: f32 = b.size.width.into();
                    if x >= roll::LABEL_WIDTH {
                        a.seek(
                            start
                                + ((x - roll::LABEL_WIDTH) / (width - roll::LABEL_WIDTH))
                                    .clamp(0., 1.) as f64
                                    * seconds,
                        );
                    }
                }),
            );
        let overview = div()
            .id("overview")
            .h(px(54.))
            .flex_shrink_0()
            .cursor_pointer()
            .border_1()
            .border_color(rgb(EDGE))
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
                    let b = overview_bounds.get();
                    let x: f32 = (e.position.x - b.origin.x).into();
                    let width: f32 = b.size.width.into();
                    a.seek((x / width).clamp(0., 1.) as f64 * a.doc().duration());
                }),
            );
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .gap_5()
                            .child(div().text_color(rgb(MINT)).child("Target"))
                            .child(div().text_color(rgb(AMBER)).child("Your voice")),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(button("zoom-in", "-", false, cx, |a, _, _| {
                                a.workspace.document.transport.visible_seconds =
                                    (a.doc().transport.visible_seconds / 1.5).max(3.);
                                a.changed();
                            }))
                            .child(label(format!(
                                "{:.0}s",
                                self.doc().transport.visible_seconds
                            )))
                            .child(button("zoom-out", "+", false, cx, |a, _, _| {
                                a.workspace.document.transport.visible_seconds =
                                    (a.doc().transport.visible_seconds * 1.5).min(120.);
                                a.changed();
                            })),
                    ),
            )
            .child(plot)
            .child(overview)
            .child(
                div()
                    .min_h(px(45.))
                    .flex_wrap()
                    .gap_3()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_5()
                            .child(
                                div()
                                    .text_color(rgb(MINT))
                                    .child(format!("Target {target_label}")),
                            )
                            .child(
                                div()
                                    .text_color(rgb(AMBER))
                                    .text_size(px(18.))
                                    .child(live_label),
                            )
                            .child(label(tuning))
                            .child(label(interval)),
                    )
                    .child(div().text_color(rgb(MUTED)).child(if !self.score_ready() {
                        "Review target to score".into()
                    } else if self.input_running && self.score.target_seconds > 0. {
                        format!(
                            "{:.0}% in tune  /  {:.0}% covered",
                            self.score.score, self.score.coverage
                        )
                    } else if self.input_running {
                        "Listening".into()
                    } else {
                        "Microphone off".into()
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(button("start", "|<", false, cx, |a, _, _| a.seek(0.)))
                            .child(button(
                                "play",
                                if self.play_pending {
                                    "Starting..."
                                } else if self.playing() {
                                    "Pause"
                                } else {
                                    "Play"
                                },
                                self.playing(),
                                cx,
                                |a, _, _| a.toggle_play(),
                            ))
                            .child(div().ml_3().child(time)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .flex_wrap()
                            .child(button(
                                "mic",
                                if self.input_pending {
                                    "Starting mic..."
                                } else if self.input_running {
                                    "Mic on"
                                } else {
                                    "Enable mic"
                                },
                                self.input_running,
                                cx,
                                |a, _, _| a.toggle_input(),
                            ))
                            .child(button(
                                "loop",
                                if self.doc().transport.loop_enabled {
                                    "Loop on"
                                } else {
                                    "Loop"
                                },
                                self.doc().transport.loop_enabled,
                                cx,
                                |a, _, _| {
                                    a.workspace.document.transport.loop_enabled =
                                        !a.doc().transport.loop_enabled;
                                    a.update_loop();
                                },
                            ))
                            .child(button(
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
                    ),
            )
    }
    fn navigation(&self, cx: &Context<Self>) -> impl IntoElement {
        let mut nav = div()
            .h(px(54.))
            .px_6()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(rgb(EDGE));
        let mut tabs = div().flex().gap_2();
        for (id, title, panel) in [
            ("mix-tab", "Mix", Panel::Mix),
            ("target-tab", "Target", Panel::Target),
            ("audio-tab", "Audio", Panel::Audio),
            ("history-tab", "History", Panel::History),
            ("help-tab", "Keys", Panel::Help),
        ] {
            tabs = tabs.child(button(
                id,
                title,
                self.panel == Some(panel),
                cx,
                move |a, _, _| {
                    a.panel = if a.panel == Some(panel) {
                        None
                    } else {
                        Some(panel)
                    }
                },
            ));
        }
        nav = nav
            .child(tabs)
            .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(
                if self.audio_unresponsive {
                    "Audio device not responding"
                } else if self.audio_starting || self.audio_configuring {
                    "Starting audio device..."
                } else {
                    "Space play / pause    Enter start"
                },
            ));
        nav
    }
    fn panel_view(&self, cx: &Context<Self>) -> AnyElement {
        let Some(panel) = self.panel else {
            return div().into_any_element();
        };
        let title = match panel {
            Panel::Mix => "Mix",
            Panel::Audio => "Audio",
            Panel::Target => "Target",
            Panel::History => "Practice history",
            Panel::Help => "Keyboard",
        };
        let mut content = div().flex().flex_col().gap_3();
        match panel {
            Panel::Mix => {
                for role in [TrackRole::Target, TrackRole::Backing] {
                    let track = self.doc().track(role);
                    let present = track.is_some();
                    let name = track.map_or(
                        if role == TrackRole::Backing {
                            "Optional"
                        } else {
                            "No target"
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
                    let prefix = role.label();
                    content = content
                        .child(section(prefix))
                        .child(div().text_size(px(11.)).overflow_hidden().child(name))
                        .child(button(
                            format!("import-{prefix}"),
                            if present { "Replace..." } else { "Import..." },
                            false,
                            cx,
                            move |a, _, c| a.choose_import(role, c),
                        ));
                    if present {
                        content = content
                            .child(row(
                                "Level",
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(button(
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
                                "remove-backing",
                                "Remove backing",
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
                    .child(label("Analysis for the next import"))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(button(
                                "mono",
                                "One voice",
                                self.mode == AnalysisMode::Monophonic,
                                cx,
                                |a, _, _| a.mode = AnalysisMode::Monophonic,
                            ))
                            .child(button(
                                "poly",
                                "Polyphonic",
                                self.mode == AnalysisMode::Polyphonic,
                                cx,
                                |a, _, _| a.mode = AnalysisMode::Polyphonic,
                            )),
                    )
                    .child(button(
                        "replace-target",
                        "Import target...",
                        false,
                        cx,
                        |a, _, c| a.choose_import(TrackRole::Target, c),
                    ));
                if self.doc().target.is_some() {
                    content = content
                        .child(button(
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
                        .child(label(format!("{} notes", self.target.notes.len())));
                }
                if self.target.mode == AnalysisMode::Polyphonic {
                    content=content.child(section("Choose a practice line")).child(label("Gray notes are candidates. Mint is the selected line. Review it by listening."))
                        .child(div().flex().gap_2().child(button("highest","Highest",self.target.selection.policy==LinePolicy::Highest,cx,|a,_,_|a.change_selection(|s|s.policy=LinePolicy::Highest))).child(button("lowest","Lowest",self.target.selection.policy==LinePolicy::Lowest,cx,|a,_,_|a.change_selection(|s|s.policy=LinePolicy::Lowest))));
                    for lower in [true, false] {
                        let midi = if lower {
                            self.target.selection.min_midi
                        } else {
                            self.target.selection.max_midi
                        };
                        content = content.child(row(
                            if lower { "Lowest note" } else { "Highest note" },
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(button(
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
                    content = content.child(button("review", if self.target.reviewed { "Line reviewed" } else { "Confirm reviewed line" }, self.target.reviewed, cx, |app,_,_|app.confirm_target()))
                        .child(label("Polyphonic notes are estimates. This does not separate singers or identify choir sections."));
                }
                content = content.child(section("Scoring")).child(row(
                    "Tolerance",
                    div()
                        .flex()
                        .gap_2()
                        .items_center()
                        .child(button("tolerance-down", "-", false, cx, |a, _, _| {
                            a.pause();
                            a.workspace.document.settings.tolerance_cents =
                                (a.doc().settings.tolerance_cents - 5.).max(10.);
                            a.changed();
                        }))
                        .child(format!("{:.0} cents", self.doc().settings.tolerance_cents))
                        .child(button("tolerance-up", "+", false, cx, |a, _, _| {
                            a.pause();
                            a.workspace.document.settings.tolerance_cents =
                                (a.doc().settings.tolerance_cents + 5.).min(100.);
                            a.changed();
                        })),
                ));
            }
            Panel::Audio => {
                content = content
                    .child(label(if self.audio_unresponsive {
                        "Audio device is not responding."
                    } else if self.audio_starting {
                        "Starting audio..."
                    } else if self.audio_configuring {
                        "Applying audio settings..."
                    } else {
                        "Choose devices, then apply settings."
                    }))
                    .child(button(
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
                    .child(section("Input device"));
                let input_selected = &self.doc().settings.audio.input_device;
                content = content.child(button(
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
                        "Input channel",
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(button("channel-down", "-", false, cx, |a, _, _| {
                                a.workspace.document.settings.audio.input_channel =
                                    a.doc().settings.audio.input_channel.saturating_sub(1);
                                a.changed();
                            }))
                            .child(format!("{}", channel + 1))
                            .child(button("channel-up", "+", false, cx, |a, _, _| {
                                a.workspace.document.settings.audio.input_channel =
                                    (a.doc().settings.audio.input_channel + 1).min(63);
                                a.changed();
                            })),
                    ))
                    .child(row(
                        "Input gain",
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(button("input-gain-down", "-", false, cx, |a, _, _| {
                                a.workspace.document.settings.audio.input_gain =
                                    (a.doc().settings.audio.input_gain - 0.1).max(0.);
                                a.changed();
                            }))
                            .child(format!("{:.1}x", self.doc().settings.audio.input_gain))
                            .child(button("input-gain-up", "+", false, cx, |a, _, _| {
                                a.workspace.document.settings.audio.input_gain =
                                    (a.doc().settings.audio.input_gain + 0.1).min(4.);
                                a.changed();
                            })),
                    ))
                    .child(section("Output device"));
                let output_selected = &self.doc().settings.audio.output_device;
                content = content.child(button(
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
                    .child(section("Buffer frames"))
                    .child(buffers)
                    .child(button(
                        "apply-audio",
                        "Apply settings",
                        true,
                        cx,
                        |a, _, _| a.apply_audio(),
                    ))
                    .child(button(
                        "monitor",
                        if self.doc().settings.audio.monitor {
                            "Monitoring on"
                        } else {
                            "Monitoring off"
                        },
                        self.doc().settings.audio.monitor,
                        cx,
                        |a, _, _| {
                            if !a.input_running {
                                a.fail("Enable the microphone before turning on monitoring.");
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
                    .child(label("Use headphones when monitoring."));
                let compensation = self.doc().settings.audio.latency_ms;
                content = content
                    .child(row(
                        "Timing correction",
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(button("latency-down", "-", false, cx, |a, _, _| {
                                a.workspace.document.settings.audio.latency_ms =
                                    (a.doc().settings.audio.latency_ms - 5.).max(-500.);
                                a.changed();
                            }))
                            .child(format!("{compensation:.0} ms"))
                            .child(button("latency-up", "+", false, cx, |a, _, _| {
                                a.workspace.document.settings.audio.latency_ms =
                                    (a.doc().settings.audio.latency_ms + 5.).min(500.);
                                a.changed();
                            })),
                    ))
                    .child(label("Positive values move captured pitch earlier."));
                if let Some(e) = &self.engine {
                    let d = e.diagnostics();
                    content = content
                        .child(section("Active stream"))
                        .child(label(format!(
                            "{} Hz  /  {} output channels",
                            d.sample_rate, d.output_channels
                        )))
                        .child(label(format!(
                            "Actual buffer: {} out / {} in",
                            d.output_buffer_frames, d.input_buffer_frames
                        )))
                        .child(label(format!(
                            "{} underruns / {} input drops",
                            d.underruns, d.input_drops
                        )));
                    if d.buffer_fallback {
                        content = content.child(label("The device chose a supported buffer."));
                    }
                    if d.stream_error {
                        content = content.child(
                            div()
                                .text_color(rgb(AMBER))
                                .child("Audio stream interrupted. Apply settings to reconnect."),
                        );
                    }
                }
            }
            Panel::History => {
                if self.doc().history.is_empty() {
                    content = content.child(label(
                        "No practice takes yet. Enable the microphone and play a reviewed target.",
                    ));
                }
                for (i, take) in self.doc().history.iter().enumerate().rev() {
                    let start = take.start;
                    content = content
                        .child(button(
                            format!("take-{i}"),
                            format!("Take {}   {:.0}%   {}", i + 1, take.score, clock(start)),
                            false,
                            cx,
                            move |a, _, _| a.seek(start),
                        ))
                        .child(label(format!(
                            "{:.0}% covered / {:.1}s evaluated",
                            take.coverage, take.evaluated_seconds
                        )));
                }
            }
            Panel::Help => {
                for (key, description) in [
                    ("Space", "Play / pause"),
                    ("Enter", "Return to start"),
                    ("Left / Right", "Seek one second"),
                    ("L", "Toggle loop"),
                    ("Cmd+I", "Import target"),
                    ("Cmd+O", "Open project"),
                    ("Cmd+S", "Save project"),
                    ("Cmd+Shift+S", "Save as"),
                    ("Cmd+N", "New project"),
                    ("Escape", "Close panel"),
                    ("Cmd+1-4", "Mix / Target / Audio / History"),
                    ("Cmd+/", "Keyboard shortcuts"),
                ] {
                    content = content.child(row(key, div().child(description)));
                }
            }
        }
        div()
            .w(px(340.))
            .flex_shrink_0()
            .h_full()
            .border_l_1()
            .border_color(rgb(EDGE))
            .bg(rgb(0x18201b))
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(16.)).child(title))
                    .child(button("close-panel", "Close", false, cx, |a, _, _| {
                        a.panel = None
                    })),
            )
            .child(
                div()
                    .id("panel-scroll")
                    .overflow_y_scroll()
                    .flex_1()
                    .min_h_0()
                    .child(content),
            )
            .into_any_element()
    }
}
impl Render for VoiceApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.busy.is_none()
            && let Ok(urls) = self.open_requests.try_recv()
            && let Some(path) = urls.first().and_then(|u| super::file_url_path(u))
        {
            self.guard_changes(window, cx, move |app, _, _| app.open_path(path));
        }
        window.set_window_title(&format!(
            "{}{} — Voice",
            self.doc().title,
            if self.dirty { " *" } else { "" }
        ));
        let body = if self.doc().target.is_some() {
            self.workspace_view(cx).into_any_element()
        } else {
            self.empty(cx).into_any_element()
        };
        let mut root = div()
            .id("voice-root")
            .on_drop(cx.listener(|a, paths: &ExternalPaths, w, c| {
                if let Some(path) = paths.paths().first() {
                    if path.extension().is_some_and(|e| e == "voice") {
                        let p = path.clone();
                        a.guard_changes(w, c, move |a, _, _| a.open_path(p));
                    } else {
                        a.import_drop(paths.paths());
                    }
                }
            }))
            .key_context("Voice")
            .on_key_down(cx.listener(|_, _, _, cx| cx.notify()))
            .on_action(cx.listener(|a, _: &ShowMix, _, cx| {
                a.panel = if a.panel == Some(Panel::Mix) {
                    None
                } else {
                    Some(Panel::Mix)
                };
                cx.notify();
            }))
            .on_action(cx.listener(|a, _: &ShowTarget, _, cx| {
                a.panel = if a.panel == Some(Panel::Target) {
                    None
                } else {
                    Some(Panel::Target)
                };
                cx.notify();
            }))
            .on_action(cx.listener(|a, _: &ShowAudio, _, cx| {
                a.panel = if a.panel == Some(Panel::Audio) {
                    None
                } else {
                    Some(Panel::Audio)
                };
                cx.notify();
            }))
            .on_action(cx.listener(|a, _: &ShowHistory, _, cx| {
                a.panel = if a.panel == Some(Panel::History) {
                    None
                } else {
                    Some(Panel::History)
                };
                cx.notify();
            }))
            .on_action(cx.listener(|a, _: &ShowHelp, _, cx| {
                a.panel = if a.panel == Some(Panel::Help) {
                    None
                } else {
                    Some(Panel::Help)
                };
                cx.notify();
            }))
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(0x121814))
            .text_color(rgb(0xd8ded5))
            .font_family("JetBrains Mono")
            .text_size(px(12.))
            .flex()
            .flex_col()
            .on_action(cx.listener(|a, _: &TogglePlayback, _, _| a.toggle_play()))
            .on_action(cx.listener(|a, _: &ReturnToStart, _, _| a.seek(0.)))
            .on_action(cx.listener(|a, _: &OpenProject, w, c| a.choose_open(w, c)))
            .on_action(cx.listener(|a, _: &SaveProject, _, c| a.choose_save(false, c)))
            .on_action(cx.listener(|a, _: &SaveProjectAs, _, c| a.choose_save(true, c)))
            .on_action(cx.listener(|a, _: &NewProject, w, c| a.new_project(w, c)))
            .on_action(
                cx.listener(|a, _: &ImportTarget, _, c| a.choose_import(TrackRole::Target, c)),
            )
            .on_action(cx.listener(|a, _: &Quit, w, c| a.quit(w, c)))
            .on_action(cx.listener(|a, _: &SeekBack, _, _| a.seek(a.cursor - 1.)))
            .on_action(cx.listener(|a, _: &SeekForward, _, _| a.seek(a.cursor + 1.)))
            .on_action(cx.listener(|a, _: &ToggleLoop, _, _| {
                a.workspace.document.transport.loop_enabled = !a.doc().transport.loop_enabled;
                a.update_loop();
            }))
            .on_action(cx.listener(|a, _: &ClosePanel, _, _| a.panel = None))
            .child(self.header(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(body)
                    .child(self.panel_view(cx)),
            );
        if let Some(busy) = &self.busy {
            let progress = self.progress.load(Ordering::Relaxed) as f32 / 10.;
            root = root.child(
                div()
                    .h(px(48.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_6()
                    .bg(rgb(0x26392c))
                    .child(format!("{busy}... {progress:.0}%"))
                    .when(busy.starts_with("Analyzing"), |s| {
                        s.child(button("cancel-job", "Cancel", false, cx, |a, _, _| {
                            a.cancel.store(true, Ordering::Relaxed);
                        }))
                    }),
            );
        }
        if let Some(notice) = &self.notice {
            root = root.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_6()
                    .py_2()
                    .bg(rgb(if self.error { 0x392b1e } else { 0x203027 }))
                    .text_color(rgb(if self.error { AMBER } else { MINT }))
                    .child(div().flex_1().child(notice.clone()))
                    .child(button("dismiss", "Dismiss", false, cx, |a, _, _| {
                        a.notice = None
                    })),
            );
        }
        root.child(self.navigation(cx))
    }
}
