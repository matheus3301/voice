mod roll;
mod views;

use gpui::{
    App, Application, Bounds, Context, FocusHandle, KeyBinding, PathPromptOptions, PromptLevel,
    Timer, Window, WindowBounds, WindowOptions, actions, prelude::*, px, size,
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
        ShowHelp
    ]
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Mix,
    Audio,
    Target,
    History,
    Help,
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
}

impl VoiceApp {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        open_requests: crossbeam_channel::Receiver<Vec<String>>,
    ) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (engine, notice) = match AudioController::new() {
            Ok(e) => (Some(e), None),
            Err(e) => (None, Some(format!("Audio unavailable: {e:#}"))),
        };
        let devices = DeviceInventory::default();
        let focus = cx.focus_handle();
        focus.focus(window);
        cx.spawn_in(window, async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(33)).await;
                if let Err(error) = this.update_in(cx, |app, window, cx| {
                    let was_busy = app.busy.is_some();
                    let animate = app.playing()
                        || app.was_playing
                        || app.input_pending
                        || app.play_pending
                        || app.audio_starting
                        || app.audio_configuring
                        || app.input_running
                        || app.busy.is_some()
                        || !app.rx.is_empty()
                        || !app.open_requests.is_empty();
                    let changed = app.tick();
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
        Self {
            workspace: ProjectWorkspace::new(),
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
                if let Some(error) = status.error {
                    self.fail(error);
                }
            }
        }
        let actual_input = self.engine.as_ref().is_some_and(|e| e.is_input_running());
        if self.input_pending && actual_input {
            self.input_pending = false;
            self.input_running = true;
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
                    if let Some(e) = self.engine.as_mut()
                        && let Err(error) =
                            e.configure_audio(&self.workspace.document.settings.audio)
                    {
                        self.fail(format!("Project opened. Audio needs attention: {error:#}"));
                    }
                    if let Err(e) = self.sync_audio() {
                        self.fail(format!("Project opened. Playback unavailable: {e:#}"));
                    }
                }
                JobResult::Saved { path, revision } => {
                    self.workspace.file_path = Some(path);
                    if revision == self.revision {
                        self.dirty = false;
                    }
                    self.error = false;
                    self.notice = Some("Project saved.".into());
                }
                JobResult::Failed(error) => {
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
            self.fail("Microphone disconnected. Open Audio to reconnect.");
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
        changed
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
            };
            self.workspace.document.history.push(take);
            self.changed();
        } else {
            self.take_trace.clear();
        }
    }
    fn pause(&mut self) {
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
    }
    fn toggle_play(&mut self) {
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
            None => self.fail("No output device is available. Open Audio to retry."),
        }
    }
    fn seek(&mut self, time: f64) {
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
    fn import_drop(&mut self, paths: &[PathBuf]) {
        if self.busy.is_some() {
            return;
        }
        if let Some(path) = paths.first() {
            self.pending_backing = paths.get(1).cloned();
            self.import_path(TrackRole::Target, path.clone());
        }
    }
    fn choose_import(&mut self, role: TrackRole, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(format!("Import {}", role.label().to_lowercase()).into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |app, _| app.import_path(role, path));
            }
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
                .unwrap_or_else(|e| JobResult::Failed(format!("Cannot open project: {e:#}")));
            let _ = tx.send(result);
        });
    }
    fn choose_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.guard_changes(window, cx, |_, _, cx| {
            let receiver = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Open project".into()),
            });
            cx.spawn(async move |this, cx| {
                if let Ok(Ok(Some(paths))) = receiver.await
                    && let Some(path) = paths.into_iter().next()
                {
                    let _ = this.update(cx, |app, _| app.open_path(path));
                }
            })
            .detach();
        });
    }
    fn save_to(&mut self, path: PathBuf) {
        let Some((tx, _, _)) = self.start_job("Saving project".into()) else {
            return;
        };
        self.workspace.document.transport.cursor = self.cursor;
        let document = self.doc().clone();
        let paths = self.workspace.media_paths.clone();
        let revision = self.revision;
        std::thread::spawn(move || {
            let result = project::save_project(&document, &paths, &path)
                .map(|_| JobResult::Saved { path, revision })
                .unwrap_or_else(|e| JobResult::Failed(format!("Cannot save project: {e:#}")));
            let _ = tx.send(result);
        });
    }
    fn choose_save(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.pause();
        if !force && let Some(path) = self.workspace.file_path.clone() {
            self.save_to(path);
            return;
        }
        let name = format!("{}.voice", self.doc().title.replace(['/', ':'], "-"));
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
        let receiver = cx.prompt_for_new_path(&directory, Some(&name));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(mut path))) = receiver.await {
                if path.extension().is_none_or(|e| e != "voice") {
                    path.set_extension("voice");
                }
                let _ = this.update(cx, |app, _| app.save_to(path));
            }
        })
        .detach();
    }
    fn guard_changes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        action: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if self.busy.is_some() {
            return;
        }
        if !self.dirty {
            action(self, window, cx);
            return;
        }
        self.pause();
        let answer = window.prompt(
            PromptLevel::Warning,
            "Discard unsaved changes?",
            Some("Save the project first to keep imported audio, settings and practice history."),
            &["Cancel", "Discard changes"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await.ok() == Some(1) {
                let _ = this.update_in(cx, |app, window, cx| action(app, window, cx));
            }
        })
        .detach();
    }
    fn new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.guard_changes(window, cx, |app, _, _| {
            app.pause();
            app.stop_input();
            app.workspace = ProjectWorkspace::new();
            app.cursor = 0.;
            app.view_start = 0.;
            app.dirty = false;
            app.panel = None;
            app.notice = None;
            app.mode = AnalysisMode::Monophonic;
            app.refresh_analysis();
            if let Err(e) = app.sync_audio() {
                app.fail(e);
            }
        });
    }
    fn quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.guard_changes(window, cx, |app, _, cx| {
            app.pending_exit = true;
            app.stop_input();
            cx.quit();
        });
    }
    fn stop_input(&mut self) {
        if let Some(e) = self.engine.as_mut() {
            e.stop_input();
        }
        self.input_running = false;
        self.input_pending = false;
        self.workspace.document.settings.audio.monitor = false;
        self.live = None;
        self.finish_take();
    }
    fn toggle_input(&mut self) {
        if self.input_running || self.input_pending {
            self.stop_input();
            return;
        }
        self.workspace.document.settings.audio.monitor = false;
        if let Some(e) = self.engine.as_mut() {
            match e.start_input(&self.workspace.document.settings.audio) {
                Ok(()) => {
                    self.input_pending = true;
                }
                Err(e) => self.fail(format!("Cannot enable microphone: {e:#}")),
            }
        } else {
            self.fail("No audio device is available.");
        }
    }
    fn apply_audio(&mut self) {
        self.pause();
        self.stop_input();
        if self.engine.is_none() {
            match AudioController::new() {
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

pub fn run() {
    let (open_tx, open_rx) = crossbeam_channel::unbounded();
    let application = Application::new();
    application.on_open_urls(move |urls| {
        let _ = open_tx.send(urls);
    });
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    application.run(move |cx: &mut App| {
        cx.text_system()
            .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
                "../../assets/fonts/JetBrainsMono-Regular.ttf"
            ))])
            .expect("Bundled font is valid");
        cx.bind_keys([
            KeyBinding::new("space", TogglePlayback, None),
            KeyBinding::new("enter", ReturnToStart, None),
            KeyBinding::new("cmd-o", OpenProject, None),
            KeyBinding::new("cmd-s", SaveProject, None),
            KeyBinding::new("cmd-shift-s", SaveProjectAs, None),
            KeyBinding::new("cmd-n", NewProject, None),
            KeyBinding::new("cmd-i", ImportTarget, None),
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-w", Quit, None),
            KeyBinding::new("left", SeekBack, None),
            KeyBinding::new("right", SeekForward, None),
            KeyBinding::new("l", ToggleLoop, None),
            KeyBinding::new("escape", ClosePanel, None),
        ]);
        cx.bind_keys([
            KeyBinding::new("cmd-1", ShowMix, None),
            KeyBinding::new("cmd-2", ShowTarget, None),
            KeyBinding::new("cmd-3", ShowAudio, None),
            KeyBinding::new("cmd-4", ShowHistory, None),
            KeyBinding::new("cmd-/", ShowHelp, None),
        ]);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1180.), px(780.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(880.), px(600.))),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Voice".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let entity = cx.new(|cx| VoiceApp::new(window, cx, open_rx));
                let weak = entity.downgrade();
                window.on_window_should_close(cx, move |window, cx| {
                    weak.update(cx, |app, cx| {
                        if app.pending_exit {
                            return true;
                        }
                        app.quit(window, cx);
                        false
                    })
                    .unwrap_or(true)
                });
                if let Some(path) = path {
                    entity.update(cx, |app, _| app.open_path(path));
                }
                entity
            },
        )
        .expect("Cannot open Voice window");
        cx.activate(true);
    });
}
