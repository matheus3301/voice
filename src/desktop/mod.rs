mod application;
use application::install_menus;
pub use application::run;
mod mixer;
mod platform;
mod roll;
mod session;
mod settings;
mod theme;
mod views;

use gpui::{
    App, Application, Bounds, Context, FocusHandle, Global, KeyBinding, Menu, MenuItem,
    PromptLevel, SystemMenuType, Timer, Window, WindowBounds, WindowHandle, WindowOptions, actions,
    prelude::*, px, size,
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use voice::{
    analysis,
    audio::DeviceInventory,
    audio_controller::AudioController,
    document_lifecycle::{DocumentIntent, DocumentLifecycle, DocumentStep, SaveChoice},
    media,
    model::*,
    project::{self, ProjectWorkspace},
    scoring::{self, PracticeScorer, ScoreSnapshot},
};

actions!(
    voice,
    [
        TogglePlayback,
        ReturnToStart,
        OpenProject,
        SaveProject,
        SaveProjectAs,
        NewProject,
        ImportTarget,
        Quit,
        SeekBack,
        SeekForward,
        ToggleLoop,
        ClosePanel,
        ShowMix,
        ShowTarget,
        ShowAudio,
        ShowHistory,
        ShowHelp,
        ImportAccompaniment,
        ToggleMicrophone,
        ToggleMonitoring,
        ToggleRecording,
        CloseDocument,
        AboutVoice,
        HideVoice,
        HideOthers,
        ShowAll,
        MinimizeWindow,
        ZoomWindow,
        EditUndo,
        EditRedo,
        EditCut,
        EditCopy,
        EditPaste,
        EditSelectAll
    ]
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Audio,
    Target,
    History,
    Help,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MicrophoneRequestOutcome {
    None,
    Start,
    Explain,
}
fn microphone_request_outcome(
    wanted: bool,
    granted: bool,
    authorization: platform::MicrophoneAuthorization,
) -> MicrophoneRequestOutcome {
    if !wanted {
        MicrophoneRequestOutcome::None
    } else if granted && authorization == platform::MicrophoneAuthorization::Authorized {
        MicrophoneRequestOutcome::Start
    } else {
        MicrophoneRequestOutcome::Explain
    }
}

enum JobResult {
    Imported {
        role: TrackRole,
        path: PathBuf,
        track: Box<MediaTrack>,
    },
    Opened(Box<ProjectWorkspace>),
    Saved {
        path: PathBuf,
        revision: u64,
    },
    Failed(String),
}

pub struct VoiceApp {
    workspace: ProjectWorkspace,
    engine: Option<AudioController>,
    devices: DeviceInventory,
    focus: FocusHandle,
    panel: Option<Panel>,
    mode: AnalysisMode,
    target: Arc<TrackAnalysis>,
    selected: Arc<TrackAnalysis>,
    live: Option<PitchFrame>,
    trace: VecDeque<PitchFrame>,
    take_trace: Vec<PitchFrame>,
    pending_scores: VecDeque<PitchFrame>,
    take_start: Option<(f64, u64)>,
    scorer: PracticeScorer,
    score: ScoreSnapshot,
    input_running: bool,
    input_pending: bool,
    play_pending: bool,
    audio_error_seq: u64,
    audio_completed_seq: u64,
    audio_starting: bool,
    audio_configuring: bool,
    audio_wait_started: Option<Instant>,
    audio_unresponsive: bool,
    cursor: f64,
    view_start: f64,
    dirty: bool,
    revision: u64,
    busy: Option<String>,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    tx: crossbeam_channel::Sender<JobResult>,
    rx: crossbeam_channel::Receiver<JobResult>,
    notice: Option<String>,
    error: bool,
    pending_exit: bool,
    pending_backing: Option<PathBuf>,
    open_requests: crossbeam_channel::Receiver<Vec<String>>,
    loop_epoch: u64,
    was_playing: bool,
    max_note_duration: f64,
    microphone_authorization: platform::MicrophoneAuthorization,
    permission_pending: bool,
    microphone_requested: bool,
    microphone_explanation_pending: bool,
    lifecycle: DocumentLifecycle,
    deferred_document_intent: Option<DocumentIntent>,
    native_dialog_open: bool,
    deferred_document_step: Option<DocumentStep>,
    native_document_state: Option<(Option<PathBuf>, bool, String)>,
    audio_settings_window: Option<WindowHandle<settings::AudioSettingsWindow>>,
    menus_need_refresh: bool,
    setup_reference: Option<PathBuf>,
    setup_accompaniment: Option<PathBuf>,
    sidebar_visible: bool,
    mixer_ui: mixer::MixerUi,
    audio_preferences: AudioPreferences,
    preference_writer: Option<crossbeam_channel::Sender<AudioPreferences>>,
    preference_results: crossbeam_channel::Receiver<Result<(), String>>,
    preferences_status: Option<String>,
    reconnect_microphone: bool,
    monitor_requested: bool,
    record_requested: bool,
    record_started_microphone: bool,
    recording: Option<session::RecordingState>,
    recording_status: Option<String>,
    replaying_take: Option<String>,
    replay_restore: Option<(f64, f64)>,
    save_after_recording: Option<bool>,
}

impl VoiceApp {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        open_requests: crossbeam_channel::Receiver<Vec<String>>,
    ) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (audio_preferences, preference_writer, preference_results, preference_notice) =
            session::load_preferences();
        let (engine, notice) = match AudioController::new_with_preferences(&audio_preferences) {
            Ok(e) => (Some(e), preference_notice),
            Err(e) => (None, Some(format!("Audio unavailable: {e:#}"))),
        };
        let devices = DeviceInventory::default();
        let focus = cx.focus_handle();
        focus.focus(window);
        cx.observe_window_activation(window, |app, window, cx| {
            app.microphone_authorization = platform::microphone_authorization();
            if app.microphone_authorization != platform::MicrophoneAuthorization::Authorized
                && app.input_running
            {
                app.stop_input();
            }
            app.sync_document_chrome(window);
            cx.notify();
        })
        .detach();
        cx.observe_window_appearance(window, |_, window, cx| {
            window.refresh();
            cx.notify();
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(33)).await;
                if let Err(error) = this.update_in(cx, |app, window, cx| {
                    let was_busy = app.busy.is_some();
                    let animate = app.playing()
                        || app.was_playing
                        || app.input_pending
                        || app.microphone_explanation_pending
                        || app.play_pending
                        || app.audio_starting
                        || app.audio_configuring
                        || app.input_running
                        || app.recording.is_some()
                        || app.record_requested
                        || app.replaying_take.is_some()
                        || !app.preference_results.is_empty()
                        || app.busy.is_some()
                        || !app.rx.is_empty()
                        || !app.open_requests.is_empty()
                        || app.deferred_document_step.is_some()
                        || app.deferred_document_intent.is_some();
                    let changed = app.tick();
                    app.advance_document_lifecycle(window, cx);
                    app.sync_document_chrome(window);
                    if app.menus_need_refresh {
                        install_menus(cx);
                        app.menus_need_refresh = false;
                    }
                    if animate || changed {
                        window.refresh();
                        if was_busy && app.busy.is_none() {
                            window.activate_window();
                            app.focus.focus(window);
                        }
                        cx.notify();
                    }
                }) {
                    eprintln!("Voice window polling stopped: {error:#}");
                    break;
                }
            }
        })
        .detach();
        let mut workspace = ProjectWorkspace::new();
        workspace.document.settings.audio = audio_preferences.clone();
        Self {
            workspace,
            engine,
            devices,
            focus,
            panel: None,
            mode: AnalysisMode::Monophonic,
            target: Arc::new(TrackAnalysis::default()),
            selected: Arc::new(TrackAnalysis::default()),
            live: None,
            trace: VecDeque::new(),
            take_trace: Vec::new(),
            pending_scores: VecDeque::new(),
            take_start: None,
            scorer: PracticeScorer::new(),
            score: ScoreSnapshot::default(),
            input_running: false,
            input_pending: false,
            play_pending: false,
            audio_error_seq: 0,
            audio_completed_seq: 0,
            audio_starting: true,
            audio_configuring: false,
            audio_wait_started: Some(Instant::now()),
            audio_unresponsive: false,
            cursor: 0.,
            view_start: 0.,
            dirty: false,
            revision: 0,
            busy: None,
            progress: Arc::new(AtomicU32::new(0)),
            cancel: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
            error: notice.is_some(),
            notice,
            pending_exit: false,
            pending_backing: None,
            open_requests,
            loop_epoch: 0,
            was_playing: false,
            max_note_duration: 0.,
            microphone_authorization: platform::microphone_authorization(),
            permission_pending: false,
            microphone_requested: false,
            microphone_explanation_pending: false,
            lifecycle: DocumentLifecycle::default(),
            deferred_document_intent: None,
            native_dialog_open: false,
            deferred_document_step: None,
            native_document_state: None,
            audio_settings_window: None,
            menus_need_refresh: false,
            setup_reference: None,
            setup_accompaniment: None,
            sidebar_visible: true,
            mixer_ui: mixer::MixerUi::default(),
            audio_preferences,
            preference_writer,
            preference_results,
            preferences_status: None,
            reconnect_microphone: false,
            monitor_requested: false,
            record_requested: false,
            record_started_microphone: false,
            recording: None,
            recording_status: None,
            replaying_take: None,
            replay_restore: None,
            save_after_recording: None,
        }
    }
    fn doc(&self) -> &VoiceProject {
        &self.workspace.document
    }
    fn changed(&mut self) {
        self.dirty = true;
        self.revision += 1;
        self.workspace.document.modified_at_ms = timestamp_ms();
    }
    fn fail(&mut self, error: impl ToString) {
        self.error = true;
        self.notice = Some(error.to_string());
    }
    fn playing(&self) -> bool {
        self.engine.as_ref().is_some_and(|e| e.is_playing())
    }
    fn score_ready(&self) -> bool {
        self.doc().target.is_some()
            && (self.target.mode == AnalysisMode::Monophonic || self.target.reviewed)
    }
    fn refresh_analysis(&mut self) {
        self.target = Arc::new(
            self.doc()
                .target
                .as_ref()
                .and_then(|t| t.analysis.clone())
                .unwrap_or_default(),
        );
        self.selected = Arc::new(analysis::practice_target(&self.target));
        self.max_note_duration = self
            .target
            .notes
            .iter()
            .chain(self.selected.notes.iter())
            .map(|n| n.end - n.start)
            .fold(0., f64::max);
        self.scorer.reset();
        self.score = ScoreSnapshot::default();
        self.trace.clear();
    }
    fn sync_audio(&mut self) -> anyhow::Result<()> {
        if let Some(e) = self.engine.as_mut() {
            e.set_tracks(
                self.workspace.media_paths.get(&TrackRole::Target).cloned(),
                self.workspace.media_paths.get(&TrackRole::Backing).cloned(),
            )?;
            let s = &self.workspace.document.settings;
            e.set_gains(
                if s.target_muted { 0. } else { s.target_gain },
                if s.backing_muted { 0. } else { s.backing_gain },
            );
            e.set_monitor_gain(s.monitor_gain);
            let t = &self.workspace.document.transport;
            e.set_loop(t.loop_start, t.loop_end, t.loop_enabled);
            e.seek(self.cursor)?;
        }
        Ok(())
    }
    fn tick(&mut self) -> bool {
        let mut changed = false;
        if let Some(engine) = &self.engine {
            let status = engine.poll();
            changed = self.audio_starting != status.initializing
                || self.audio_configuring != status.configuring
                || self.audio_error_seq != status.error_seq
                || self.audio_completed_seq != status.completed_seq;
            self.audio_completed_seq = status.completed_seq;
            self.audio_starting = status.initializing;
            self.audio_configuring = status.configuring;
            let unresponsive = if self.audio_starting || self.audio_configuring {
                self.audio_wait_started
                    .get_or_insert_with(Instant::now)
                    .elapsed()
                    > Duration::from_secs(10)
            } else {
                self.audio_wait_started = None;
                false
            };
            changed |= unresponsive != self.audio_unresponsive;
            self.audio_unresponsive = unresponsive;
            self.devices = status.devices;
            if status.error_seq != self.audio_error_seq {
                self.audio_error_seq = status.error_seq;
                self.input_pending = false;
                self.play_pending = false;
                self.record_requested = false;
                self.reconnect_microphone = false;
                if let Some(error) = status.error {
                    self.fail(error);
                }
            }
        }
        let actual_input = self.engine.as_ref().is_some_and(|e| e.is_input_running());
        if self.input_pending && actual_input {
            self.input_pending = false;
            self.input_running = true;
            if self.monitor_requested {
                self.workspace.document.settings.audio.monitor = true;
                if let Some(engine) = &self.engine {
                    engine.set_monitor(true);
                }
                self.monitor_requested = false;
            }
            if self.playing() {
                self.begin_take(self.cursor);
            }
        }

        while let Ok(result) = self.rx.try_recv() {
            self.busy = None;
            match result {
                JobResult::Imported { role, path, track } => {
                    if self.cancel.load(Ordering::Relaxed) {
                        self.pending_backing = None;
                        self.error = false;
                        self.notice =
                            Some("Import canceled. The current project was preserved.".into());
                        continue;
                    }
                    self.pause();
                    if role == TrackRole::Target {
                        if self.doc().title == "Untitled" {
                            self.workspace.document.title = Path::new(&track.filename)
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                        }
                        self.workspace.document.target = Some(*track);
                        self.cursor = 0.;
                        self.view_start = 0.;
                        self.refresh_analysis();
                    } else {
                        self.workspace.document.backing = Some(*track);
                    }
                    self.workspace.bind_media(role, path);
                    self.workspace.document.transport.loop_end = self.doc().duration().min(10.);
                    self.changed();
                    if let Err(e) = self.sync_audio() {
                        self.fail(format!("Cannot prepare playback: {e:#}"));
                    }
                    if role == TrackRole::Target
                        && let Some(path) = self.pending_backing.take()
                    {
                        self.import_path(TrackRole::Backing, path);
                    }
                }
                JobResult::Opened(workspace) => {
                    self.pause();
                    self.stop_input();
                    self.workspace = *workspace;
                    self.workspace.document.settings.audio = self.audio_preferences.clone();
                    if let Some(path) = &self.workspace.file_path {
                        platform::note_recent_document(path);
                        self.menus_need_refresh = true;
                    }
                    self.workspace.document.transport.visible_seconds =
                        self.doc().transport.visible_seconds.clamp(3., 120.);
                    self.workspace.document.settings.audio.monitor = false;
                    self.cursor = self.doc().transport.cursor.min(self.doc().duration());
                    self.view_start = (self.cursor - 2.).max(0.);
                    self.mode = self
                        .doc()
                        .target
                        .as_ref()
                        .and_then(|t| t.analysis.as_ref())
                        .map_or(AnalysisMode::Monophonic, |a| a.mode);
                    self.refresh_analysis();
                    self.dirty = false;
                    self.revision += 1;
                    self.panel = None;
                    if let Err(e) = self.sync_audio() {
                        self.fail(format!("Project opened. Playback unavailable: {e:#}"));
                    }
                }
                JobResult::Saved { path, revision } => {
                    self.workspace.file_path = Some(path);
                    if revision == self.revision {
                        self.dirty = false;
                    }
                    if let Some(name) = self
                        .workspace
                        .file_path
                        .as_ref()
                        .and_then(|p| p.file_stem())
                    {
                        self.workspace.document.title = name.to_string_lossy().into_owned();
                    }
                    platform::note_recent_document(
                        self.workspace
                            .file_path
                            .as_ref()
                            .expect("A saved project has a path"),
                    );
                    self.menus_need_refresh = true;
                    self.deferred_document_step = Some(self.lifecycle.saved(true, self.dirty));
                    self.error = false;
                    self.notice = Some("Project saved.".into());
                }
                JobResult::Failed(error) => {
                    self.deferred_document_step = Some(self.lifecycle.saved(false, self.dirty));
                    self.pending_backing = None;
                    self.fail(error);
                }
            }
        }
        let was_active = self.take_start.is_some();
        let (position, playing, frames) = self
            .engine
            .as_ref()
            .map(|e| (e.position(), e.is_playing(), e.drain_pitch()))
            .unwrap_or((self.cursor, false, Vec::new()));
        let epoch = self.engine.as_ref().map_or(0, |e| e.loop_iteration());
        if playing {
            self.play_pending = false;
            if !self.was_playing {
                self.begin_take(position);
            }
            if epoch != self.loop_epoch {
                self.cursor = self.doc().transport.loop_end.min(self.doc().duration());
                self.finish_take();
                self.begin_take(position);
                self.trace.clear();
            }
            self.cursor = position.min(self.doc().duration());
            let seconds = self.doc().transport.visible_seconds;
            if self.cursor > self.view_start + seconds * 0.8 || self.cursor < self.view_start {
                self.view_start = (self.cursor - seconds * 0.2).max(0.);
            }
        }
        self.loop_epoch = epoch;
        if !playing && self.was_playing {
            self.cursor = position.min(self.doc().duration());
        }
        self.was_playing = playing;
        if self.input_running && self.engine.as_ref().is_some_and(|e| !e.is_input_running()) {
            self.input_running = false;
            self.workspace.document.settings.audio.monitor = false;
            self.finish_take();
            self.fail("Microphone disconnected. Open Audio Settings to reconnect.");
        }
        for frame in frames {
            self.live = Some(frame);
            if playing || was_active {
                self.trace.push_back(frame);
                if self.take_start.is_some() && frame.time >= self.take_start.map_or(0., |t| t.0) {
                    self.pending_scores.push_back(frame);
                }
            }
        }
        while self
            .pending_scores
            .front()
            .is_some_and(|f| f.time <= self.cursor)
        {
            let frame = self
                .pending_scores
                .pop_front()
                .expect("A pending frame exists");
            if self.take_start.is_some() {
                self.take_trace.push(frame);
                self.score = self.scorer.sample(
                    &self.selected.frames,
                    frame.time,
                    frame,
                    self.doc().settings.tolerance_cents,
                );
            }
        }
        while self
            .trace
            .front()
            .is_some_and(|f| f.time < self.view_start - 1.)
        {
            self.trace.pop_front();
        }
        if playing && self.take_start.is_some() {
            let delay = self
                .engine
                .as_ref()
                .map_or(0.15, |e| e.diagnostics().analysis_delay_seconds);
            let start = self.take_start.map_or(self.cursor, |t| t.0);
            self.score = self.scorer.advance(
                &self.selected.frames,
                scoring::evaluated_position(self.cursor, start, delay),
            );
        }
        if !playing && was_active {
            self.finish_take();
        }
        changed | self.tick_session()
    }
    fn begin_take(&mut self, at: f64) {
        if self.input_running && self.score_ready() {
            self.scorer.reset();
            self.scorer.begin_segment(at);
            self.score = ScoreSnapshot::default();
            self.take_trace.clear();
            self.pending_scores.clear();
            self.take_start = Some((at, timestamp_ms()));
        }
    }
    fn finish_take(&mut self) {
        let Some((start, started_at_ms)) = self.take_start.take() else {
            return;
        };
        self.pending_scores.clear();
        let end = self.cursor.max(start);
        self.take_trace.retain(|f| f.time >= start && f.time <= end);
        let score = self.scorer.snapshot();
        if score.target_seconds >= 0.25 && !self.take_trace.is_empty() {
            let take = PracticeTake {
                id: new_id(),
                started_at_ms,
                target_id: self
                    .doc()
                    .target
                    .as_ref()
                    .map_or(String::new(), |t| t.id.clone()),
                start,
                end,
                score: score.score,
                coverage: score.coverage,
                evaluated_seconds: score.target_seconds,
                trace: std::mem::take(&mut self.take_trace),
                recording: None,
                reference_selection: Some(self.target.selection.clone()),
            };
            if let Some(recording) = &mut self.recording {
                let id = recording.take.id.clone();
                let start = recording.take.start;
                let started_at_ms = recording.take.started_at_ms;
                recording.take = take;
                recording.take.id = id;
                recording.take.start = start;
                recording.take.started_at_ms = started_at_ms;
            } else {
                self.workspace.document.history.push(take);
                self.changed();
            }
        } else {
            self.take_trace.clear();
        }
    }
    fn pause(&mut self) {
        self.stop_take_playback();
        let was_playing = self.playing() || self.was_playing;
        if let Some(e) = &self.engine {
            let position = e.pause();
            if was_playing {
                self.cursor = position.min(self.doc().duration());
            }
        }
        self.play_pending = false;
        self.was_playing = false;
        self.finish_take();
        self.stop_recording_capture();
    }
    fn toggle_play(&mut self) {
        if self.replaying_take.is_some() {
            self.stop_take_playback();
            return;
        }
        if self.playing() || self.play_pending {
            self.pause();
            return;
        }
        if self.doc().target.is_none() || self.busy.is_some() {
            return;
        }
        if self.audio_unresponsive {
            self.fail(
                "The audio device is not responding. Check the interface and macOS audio output.",
            );
            return;
        }
        if self.cursor >= self.doc().duration() - 0.01 {
            self.cursor = 0.;
        }
        match self.engine.as_ref().map(|e| e.play(self.cursor)) {
            Some(Ok(())) => self.play_pending = true,
            Some(Err(e)) => self.fail(format!("Cannot play: {e:#}")),
            None => self.fail("No output device is available. Open Audio Settings to retry."),
        }
    }
    fn seek(&mut self, time: f64) {
        if self.recording_active() || self.record_requested {
            self.pause();
        }
        self.stop_take_playback();
        let playing = self.playing();
        self.finish_take();
        self.cursor = time.clamp(0., self.doc().duration());
        self.trace.clear();
        if let Some(e) = &self.engine
            && let Err(error) = e.seek(self.cursor)
        {
            self.fail(format!("Cannot seek: {error:#}"));
        }
        self.view_start = (self.cursor - self.doc().transport.visible_seconds * 0.2).max(0.);
        if playing {
            self.begin_take(self.cursor);
        }
        self.workspace.document.transport.cursor = self.cursor;
        self.changed();
    }
    fn start_job(
        &mut self,
        label: String,
    ) -> Option<(
        crossbeam_channel::Sender<JobResult>,
        Arc<AtomicBool>,
        Arc<AtomicU32>,
    )> {
        if self.busy.is_some() {
            return None;
        }
        self.pause();
        self.busy = Some(label);
        self.notice = None;
        self.error = false;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.progress.store(0, Ordering::Relaxed);
        Some((self.tx.clone(), self.cancel.clone(), self.progress.clone()))
    }
    fn import_path(&mut self, role: TrackRole, path: PathBuf) {
        let Some((tx, cancel, progress)) =
            self.start_job(format!("Analyzing {}", role.label().to_lowercase()))
        else {
            return;
        };
        let mode = self.mode;
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<JobResult> {
                let metadata = media::inspect(&path)?;
                anyhow::ensure!(
                    metadata.duration > 0.,
                    "Audio duration could not be read. Try WAV, FLAC, MP3 or M4A."
                );
                let analysis = if role == TrackRole::Target {
                    Some(analysis::analyze_file(&path, mode, cancel.clone(), |v| {
                        progress.store((v.clamp(0., 1.) * 1000.) as u32, Ordering::Relaxed);
                    })?)
                } else {
                    None
                };
                anyhow::ensure!(
                    !cancel.load(Ordering::Relaxed),
                    "Import canceled. The current project was preserved."
                );
                let id = new_id();
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let archive_path = format!("media/{id}/{filename}");
                Ok(JobResult::Imported {
                    role,
                    path,
                    track: Box::new(MediaTrack {
                        id,
                        role,
                        filename,
                        archive_path,
                        metadata,
                        analysis,
                    }),
                })
            })();
            let _ = tx.send(result.unwrap_or_else(|e| JobResult::Failed(format!("{e:#}"))));
        });
    }
    fn import_drop(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some()
            || self.native_dialog_open
            || self.recording.is_some()
            || self.record_requested
        {
            return;
        }
        if self.doc().target.is_none() {
            self.setup_reference = paths.first().cloned();
            self.setup_accompaniment = paths.get(1).cloned();
            cx.notify();
            return;
        }
        if let Some(path) = paths.first() {
            self.pending_backing = paths.get(1).cloned();
            self.choose_reference_import_mode(path.clone(), window, cx);
        }
    }
    fn choose_reference_import_mode(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.native_dialog_open = true;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let message = format!("What does “{name}” contain?");
        let answer=window.prompt(PromptLevel::Info,&message,Some("Choose Several Voices or Notes for layered harmony vocals or chords. This helps Voice find the vocal part you want to learn."),&["One Voice","Several Voices or Notes","Cancel"],cx);
        cx.spawn_in(window, async move |this, cx| {
            let response = answer.await.ok();
            let _ = this.update_in(cx, |app, window, cx| {
                app.native_dialog_open = false;
                match response {
                    Some(0) => {
                        app.mode = AnalysisMode::Monophonic;
                        app.import_path(TrackRole::Target, path);
                    }
                    Some(1) => {
                        app.mode = AnalysisMode::Polyphonic;
                        app.import_path(TrackRole::Target, path);
                    }
                    _ => app.pending_backing = None,
                }
                app.focus.focus(window);
                cx.notify();
            });
        })
        .detach();
    }
    fn choose_import(&mut self, role: TrackRole, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some()
            || self.native_dialog_open
            || self.recording.is_some()
            || self.record_requested
        {
            return;
        }
        if self.doc().target.is_none() {
            self.choose_setup_audio(role, window, cx);
            return;
        }
        self.native_dialog_open = true;
        self.panel = None;
        let receiver =
            platform::open_audio_dialog(window, &format!("Choose {}", role.label().to_lowercase()));
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.native_dialog_open = false;
                match result {
                    Ok(Ok(Some(path))) => {
                        if role == TrackRole::Target {
                            app.choose_reference_import_mode(path, window, cx);
                        } else {
                            app.import_path(role, path);
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => app.fail(format!("Couldn't open the audio file: {error:#}")),
                    Err(_) => app.fail("The file dialog closed unexpectedly."),
                }
                app.focus.focus(window);
                cx.notify();
            });
        })
        .detach();
    }
    fn open_path(&mut self, path: PathBuf) {
        let Some((tx, _, _)) = self.start_job("Opening project".into()) else {
            return;
        };
        std::thread::spawn(move || {
            let result = ProjectWorkspace::open(&path)
                .map(|workspace| JobResult::Opened(Box::new(workspace)))
                .unwrap_or_else(|error| {
                    JobResult::Failed(format!("Couldn't open the project: {error:#}"))
                });
            let _ = tx.send(result);
        });
    }
    fn choose_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_document_action(DocumentIntent::OpenDialog, window, cx);
    }
    fn show_open_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.native_dialog_open = true;
        self.panel = None;
        let receiver = platform::open_project_dialog(window);
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.native_dialog_open = false;
                match result {
                    Ok(Ok(Some(path))) => app.open_path(path),
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => app.fail(format!("Couldn't open the project: {error:#}")),
                    Err(_) => app.fail("The file dialog closed unexpectedly."),
                }
                app.focus.focus(window);
                cx.notify();
            });
        })
        .detach();
    }
    fn save_to(&mut self, path: PathBuf) {
        let Some((tx, _, _)) = self.start_job("Saving project".into()) else {
            return;
        };
        self.workspace.document.transport.cursor = self.cursor;
        let mut document = self.doc().clone();
        if let Some(name) = path.file_stem() {
            document.title = name.to_string_lossy().into_owned();
        }
        let paths = self.workspace.media_paths.clone();
        let recordings = self.workspace.recording_paths.clone();
        let revision = self.revision;
        std::thread::spawn(move || {
            let result =
                project::save_project_with_recordings(&document, &paths, &recordings, &path)
                    .map(|_| JobResult::Saved { path, revision })
                    .unwrap_or_else(|error| {
                        JobResult::Failed(format!("Couldn't save the project: {error:#}"))
                    });
            let _ = tx.send(result);
        });
    }
    fn choose_save(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() || self.native_dialog_open {
            return;
        }
        self.pause();
        if self.recording.is_some() {
            self.save_after_recording = Some(force);
            return;
        }
        if !force && let Some(path) = self.workspace.file_path.clone() {
            self.save_to(path);
            return;
        }
        self.native_dialog_open = true;
        let name = format!("{}.voice", self.document_title().replace(['/', ':'], "-"));
        let directory = self
            .workspace
            .file_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join("Documents")
            });
        let receiver = platform::save_project_dialog(window, &directory, &name);
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.native_dialog_open = false;
                match result {
                    Ok(Ok(Some(path))) => app.save_to(path),
                    Ok(Ok(None)) => app.lifecycle.cancel(),
                    Ok(Err(error)) => {
                        app.lifecycle.cancel();
                        app.fail(format!("Couldn't choose a save location: {error:#}"));
                    }
                    Err(_) => {
                        app.lifecycle.cancel();
                        app.fail("The save dialog closed unexpectedly.");
                    }
                }
                app.focus.focus(window);
                cx.notify();
            });
        })
        .detach();
    }
    fn request_document_action(
        &mut self,
        intent: DocumentIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.native_dialog_open {
            return;
        }
        if self.recording.is_some() || self.record_requested {
            self.pause();
            self.deferred_document_intent = Some(intent);
            return;
        }
        if self.busy.is_some() {
            self.deferred_document_intent = Some(intent);
            if self
                .busy
                .as_ref()
                .is_some_and(|label| label.starts_with("Analyzing"))
            {
                self.cancel.store(true, Ordering::Relaxed);
            }
            return;
        }
        self.pause();
        let step = self.lifecycle.request(intent, self.dirty);
        self.perform_document_step(step, window, cx);
    }
    fn advance_document_lifecycle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.native_dialog_open || self.busy.is_some() || self.recording.is_some() {
            return;
        }
        if let Some(force) = self.save_after_recording.take() {
            self.choose_save(force, window, cx);
            return;
        }
        if let Some(step) = self.deferred_document_step.take() {
            self.perform_document_step(step, window, cx);
        } else if let Some(intent) = self.deferred_document_intent.take() {
            self.request_document_action(intent, window, cx);
        } else if let Ok(urls) = self.open_requests.try_recv()
            && let Some(path) = urls.first().and_then(|url| file_url_path(url))
        {
            self.request_document_action(DocumentIntent::Open(path), window, cx);
        } else if self.microphone_explanation_pending {
            self.microphone_explanation_pending = false;
            self.explain_microphone_access(window, cx);
        }
    }
    fn perform_document_step(
        &mut self,
        step: DocumentStep,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match step {
            DocumentStep::None => {}
            DocumentStep::Save => self.choose_save(false, window, cx),
            DocumentStep::PromptSaveChanges => {
                self.native_dialog_open = true;
                let message = format!(
                    "Do you want to save the changes made to “{}”?",
                    self.document_title()
                );
                let answer = window.prompt(
                    PromptLevel::Warning,
                    &message,
                    Some("Your changes will be lost if you don't save them."),
                    &["Save", "Cancel", "Don't Save"],
                    cx,
                );
                cx.spawn_in(window, async move |this, cx| {
                    let choice = match answer.await.ok() {
                        Some(0) => SaveChoice::Save,
                        Some(2) => SaveChoice::Discard,
                        _ => SaveChoice::Cancel,
                    };
                    let _ = this.update_in(cx, |app, window, cx| {
                        app.native_dialog_open = false;
                        let step = app.lifecycle.respond(choice);
                        app.perform_document_step(step, window, cx);
                        cx.notify();
                    });
                })
                .detach();
            }
            DocumentStep::Perform(intent) => match intent {
                DocumentIntent::OpenDialog => self.show_open_dialog(window, cx),
                DocumentIntent::Open(path) => self.open_path(path),
                DocumentIntent::New => {
                    self.pause();
                    self.stop_input();
                    self.workspace = ProjectWorkspace::new();
                    self.workspace.document.settings.audio = self.audio_preferences.clone();
                    self.setup_reference = None;
                    self.setup_accompaniment = None;
                    self.recording_status = None;
                    self.cursor = 0.;
                    self.view_start = 0.;
                    self.dirty = false;
                    self.panel = None;
                    self.notice = None;
                    self.mode = AnalysisMode::Monophonic;
                    self.refresh_analysis();
                    if let Err(error) = self.sync_audio() {
                        self.fail(error);
                    }
                    self.sync_document_chrome(window);
                }
                DocumentIntent::Close => {
                    self.pending_exit = true;
                    self.stop_input();
                    if let Some(settings) = self.audio_settings_window.take() {
                        let _ = settings.update(cx, |_, window, _| window.remove_window());
                    }
                    window.remove_window();
                }
                DocumentIntent::Quit => {
                    self.pending_exit = true;
                    self.stop_input();
                    platform::allow_termination();
                    cx.quit();
                }
            },
        }
    }
    fn new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_document_action(DocumentIntent::New, window, cx);
    }
    fn quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_document_action(DocumentIntent::Quit, window, cx);
    }
    fn close_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_document_action(DocumentIntent::Close, window, cx);
    }
    fn document_title(&self) -> String {
        self.workspace
            .file_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.doc().title.clone())
    }
    fn sync_document_chrome(&mut self, window: &mut Window) {
        let title = self.document_title();
        let state = (self.workspace.file_path.clone(), self.dirty, title.clone());
        if self.native_document_state.as_ref() != Some(&state) {
            window.set_window_title(&format!("{title} — Voice"));
            platform::update_document_state(
                window,
                self.workspace.file_path.as_deref(),
                self.dirty,
            );
            self.native_document_state = Some(state);
        }
    }
    fn show_audio_settings(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        // Opening a native window can synchronously render it or fire activation
        // observers. Release the document borrow before either reads its owner.
        let owner = cx.entity();
        let existing = self.audio_settings_window;
        cx.defer(move |cx| {
            if let Some(handle) = existing
                && handle
                    .update(cx, |_, window, _| window.activate_window())
                    .is_ok()
            {
                return;
            }
            match settings::open(owner.clone(), cx) {
                Ok(handle) => {
                    owner.update(cx, |app, _| app.audio_settings_window = Some(handle));
                    let _ = handle.update(cx, |_, window, _| window.activate_window());
                }
                Err(error) => owner.update(cx, |app, cx| {
                    app.fail(format!("Couldn't open Audio Settings: {error:#}"));
                    cx.notify();
                }),
            }
        });
    }
    fn stop_input(&mut self) {
        self.stop_recording_capture();
        if let Some(e) = self.engine.as_mut() {
            e.stop_input();
        }
        self.input_running = false;
        self.input_pending = false;
        self.microphone_requested = false;
        self.microphone_explanation_pending = false;
        self.monitor_requested = false;
        self.reconnect_microphone = false;
        self.workspace.document.settings.audio.monitor = false;
        self.live = None;
        self.finish_take();
    }
    fn toggle_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input_running || self.input_pending {
            self.stop_input();
            return;
        }
        if self.permission_pending {
            self.microphone_requested = false;
            self.record_requested = false;
            self.monitor_requested = false;
            self.reconnect_microphone = false;
            return;
        }
        self.microphone_authorization = platform::microphone_authorization();
        match self.microphone_authorization {
            platform::MicrophoneAuthorization::Authorized => self.start_authorized_input(),
            platform::MicrophoneAuthorization::NotDetermined => {
                self.permission_pending = true;
                self.microphone_requested = true;
                let receiver = platform::request_microphone_authorization();
                cx.spawn(async move |this, cx| {
                    let granted = receiver.await.unwrap_or(false);
                    let _ = this.update(cx, |app, cx| {
                        app.permission_pending = false;
                        app.microphone_authorization = platform::microphone_authorization();
                        let wanted = std::mem::take(&mut app.microphone_requested);
                        match microphone_request_outcome(
                            wanted,
                            granted,
                            app.microphone_authorization,
                        ) {
                            MicrophoneRequestOutcome::Start => app.start_authorized_input(),
                            MicrophoneRequestOutcome::Explain => {
                                app.microphone_explanation_pending = true;
                                app.record_requested = false;
                                app.monitor_requested = false;
                                app.reconnect_microphone = false;
                            }
                            MicrophoneRequestOutcome::None => {}
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            _ => {
                self.record_requested = false;
                self.reconnect_microphone = false;
                self.monitor_requested = false;
                self.explain_microphone_access(window, cx);
            }
        }
    }
    fn explain_microphone_access(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.microphone_authorization == platform::MicrophoneAuthorization::Restricted {
            drop(window.prompt(
                PromptLevel::Info,
                "Microphone access is restricted",
                Some("A system restriction is preventing Voice from using the microphone."),
                &["OK"],
                cx,
            ));
            return;
        }
        let answer=window.prompt(PromptLevel::Info,"Allow microphone access for Voice",Some("Open System Settings → Privacy & Security → Microphone and turn on Voice. Your recordings can still be opened and played without microphone access."),&["Open System Settings","Cancel"],cx);
        cx.spawn(async move |_, cx| {
            if answer.await.ok() == Some(0) {
                let _ = cx.update(platform::open_microphone_settings);
            }
        })
        .detach();
    }
    fn open_microphone_settings(&mut self, cx: &mut Context<Self>) {
        platform::open_microphone_settings(cx);
    }
    fn start_authorized_input(&mut self) {
        self.workspace.document.settings.audio.monitor = false;
        if self.audio_starting || self.audio_configuring {
            self.reconnect_microphone = true;
            return;
        }
        if let Some(engine) = self.engine.as_mut() {
            match engine.start_input(&self.workspace.document.settings.audio) {
                Ok(()) => self.input_pending = true,
                Err(error) => {
                    self.record_requested = false;
                    self.monitor_requested = false;
                    self.fail(format!("Couldn't start the microphone: {error:#}"));
                }
            }
        } else {
            self.fail("No audio device is available. Open Audio Settings to reconnect.");
        }
    }
    fn toggle_monitoring(&mut self) {
        if !self.input_running {
            self.fail("Turn on the microphone before enabling monitoring.");
            return;
        }
        self.workspace.document.settings.audio.monitor = !self.doc().settings.audio.monitor;
        if let Some(engine) = &self.engine {
            engine.set_monitor(self.doc().settings.audio.monitor);
        }
        self.changed();
    }
    fn apply_audio(&mut self) {
        let reconnect = self.input_running || self.input_pending || self.reconnect_microphone;
        let monitor = self.doc().settings.audio.monitor || self.monitor_requested;
        self.pause();
        self.stop_input();
        if self.engine.is_none() {
            match AudioController::new_with_preferences(&self.workspace.document.settings.audio) {
                Ok(e) => self.engine = Some(e),
                Err(e) => {
                    self.fail(e);
                    return;
                }
            }
        }
        if let Some(e) = self.engine.as_mut()
            && let Err(error) = e.configure_audio(&self.workspace.document.settings.audio)
        {
            self.fail(format!("Cannot apply audio settings: {error:#}"));
            return;
        }
        if let Err(e) = self.sync_audio() {
            self.fail(e);
            return;
        }
        self.reconnect_microphone = reconnect;
        self.monitor_requested = monitor;
        self.persist_audio_preferences();
        self.changed();
        self.error = false;
        self.notice = None;
    }
    fn update_mix(&mut self) {
        if let Some(e) = &self.engine {
            let s = &self.doc().settings;
            e.set_gains(
                if s.target_muted { 0. } else { s.target_gain },
                if s.backing_muted { 0. } else { s.backing_gain },
            );
        }
        self.changed();
    }
    fn update_loop(&mut self) {
        if self.recording_active() || self.record_requested {
            self.pause();
        }
        let playing = self.playing();
        self.finish_take();
        self.trace.clear();
        let t = &self.doc().transport;
        if let Some(e) = &self.engine {
            e.set_loop(t.loop_start, t.loop_end, t.loop_enabled);
        }
        self.loop_epoch = self.engine.as_ref().map_or(0, |e| e.loop_iteration());
        if playing {
            self.begin_take(self.cursor);
        }
        self.changed();
    }
    fn confirm_target(&mut self) {
        self.pause();
        if self.selected.notes.is_empty() {
            self.fail("No notes in this register. Adjust the range before confirming.");
            return;
        }
        if let Some(analysis) = self
            .workspace
            .document
            .target
            .as_mut()
            .and_then(|t| t.analysis.as_mut())
        {
            analysis.reviewed = true;
        }
        self.refresh_analysis();
        self.changed();
    }
    fn change_selection(&mut self, f: impl FnOnce(&mut TargetSelection)) {
        self.pause();
        if let Some(a) = self
            .workspace
            .document
            .target
            .as_mut()
            .and_then(|t| t.analysis.as_mut())
        {
            f(&mut a.selection);
            a.reviewed = false;
        }
        self.refresh_analysis();
        self.changed();
    }
}

fn file_url_path(value: &str) -> Option<PathBuf> {
    let raw = value.strip_prefix("file://")?;
    let raw = raw.strip_prefix("localhost").unwrap_or(raw);
    if !raw.starts_with('/') {
        return None;
    }
    let mut bytes = Vec::with_capacity(raw.len());
    let source = raw.as_bytes();
    let mut index = 0;
    while index < source.len() {
        if source[index] == b'%' {
            let hex = std::str::from_utf8(source.get(index + 1..index + 3)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            bytes.push(source[index]);
            index += 1;
        }
    }
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

#[cfg(test)]
mod permission_tests {
    use super::platform::MicrophoneAuthorization;
    use super::{MicrophoneRequestOutcome, microphone_request_outcome};

    #[test]
    fn canceled_request_never_starts_input_after_a_late_permission_grant() {
        assert_eq!(
            microphone_request_outcome(false, true, MicrophoneAuthorization::Authorized),
            MicrophoneRequestOutcome::None
        );
    }
    #[test]
    fn permission_completion_requires_current_authorization_before_starting_input() {
        assert_eq!(
            microphone_request_outcome(true, true, MicrophoneAuthorization::Authorized),
            MicrophoneRequestOutcome::Start
        );
        for authorization in [
            MicrophoneAuthorization::Denied,
            MicrophoneAuthorization::Restricted,
            MicrophoneAuthorization::NotDetermined,
        ] {
            assert_eq!(
                microphone_request_outcome(true, true, authorization),
                MicrophoneRequestOutcome::Explain
            );
        }
        assert_eq!(
            microphone_request_outcome(true, false, MicrophoneAuthorization::Denied),
            MicrophoneRequestOutcome::Explain
        );
    }
}
