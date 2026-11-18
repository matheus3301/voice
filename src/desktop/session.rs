//! Practice recording, machine-local routing and the initial media selection.
use super::*;
use voice::{preferences, recording::RecordingSession};

pub(super) struct RecordingState {
    pub session: RecordingSession,
    pub take: PracticeTake,
    archive_path: String,
    finalizing: bool,
}

type PreferenceChannels = (
    AudioPreferences,
    Option<crossbeam_channel::Sender<AudioPreferences>>,
    crossbeam_channel::Receiver<Result<(), String>>,
    Option<String>,
);

fn comparison_problem(
    take: &PracticeTake,
    reference_id: Option<&str>,
    reference: &TrackAnalysis,
) -> Option<&'static str> {
    if reference_id != Some(take.target_id.as_str()) {
        Some(
            "This take belongs to a different vocal reference. Open its original project to compare it.",
        )
    } else if take.evaluated_seconds > 0.0
        && reference.mode == AnalysisMode::Polyphonic
        && take.reference_selection.as_ref() != Some(&reference.selection)
    {
        Some(
            "This take used a different reference line. Restore its note range and higher/lower selection before comparing it.",
        )
    } else {
        None
    }
}

pub(super) fn load_preferences() -> PreferenceChannels {
    let (results, receiver) = crossbeam_channel::unbounded();
    let path = match std::env::var_os("VOICE_PREFERENCES_PATH")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(preferences::default_path)
    {
        Ok(path) => path,
        Err(error) => {
            return (
                AudioPreferences::default(),
                None,
                receiver,
                Some(error.to_string()),
            );
        }
    };
    let (audio, notice) = match preferences::load(&path) {
        Ok(audio) => (audio, None),
        Err(error) => (
            AudioPreferences::default(),
            Some(format!("Couldn't restore audio devices: {error:#}")),
        ),
    };
    let (sender, updates) = crossbeam_channel::unbounded::<AudioPreferences>();
    // A single writer preserves selection order and coalesces rapid changes.
    let _ = std::thread::Builder::new()
        .name("voice-preferences".into())
        .spawn(move || {
            while let Ok(mut audio) = updates.recv() {
                while let Ok(latest) = updates.try_recv() {
                    audio = latest;
                }
                let result = preferences::save(&path, &audio)
                    .map_err(|error| format!("Couldn't remember audio devices: {error:#}"));
                let _ = results.send(result);
            }
        });
    (audio, Some(sender), receiver, notice)
}

impl VoiceApp {
    pub(super) fn persist_audio_preferences(&mut self) {
        self.audio_preferences = self.doc().settings.audio.clone();
        self.audio_preferences.monitor = false;
        if let Some(writer) = &self.preference_writer
            && writer.send(self.audio_preferences.clone()).is_err()
        {
            self.preferences_status = Some("Audio preferences could not be saved.".into());
        }
    }

    pub(super) fn select_audio_device(&mut self, input: bool, name: Option<String>) {
        let audio = &mut self.workspace.document.settings.audio;
        let selected = if input {
            &mut audio.input_device
        } else {
            &mut audio.output_device
        };
        if *selected == name {
            return;
        }
        *selected = name;
        if input {
            audio.input_channel = 0;
            self.apply_input_preferences();
        } else {
            self.apply_audio();
        }
    }

    pub(super) fn select_input_channel(&mut self, channel: u16) {
        if channel < self.available_input_channels()
            && channel != self.doc().settings.audio.input_channel
        {
            self.workspace.document.settings.audio.input_channel = channel;
            self.apply_input_preferences();
        }
    }

    fn apply_input_preferences(&mut self) {
        let reconnect = self.input_running || self.input_pending;
        let monitor = self.doc().settings.audio.monitor || self.monitor_requested;
        self.pause();
        self.stop_input();
        self.reconnect_microphone = reconnect;
        self.monitor_requested = monitor;
        self.persist_audio_preferences();
        self.changed();
    }

    pub(super) fn available_input_channels(&self) -> u16 {
        let requested = &self.doc().settings.audio.input_device;
        let diagnostics = self.engine.as_ref().map(|engine| engine.diagnostics());
        let observed = diagnostics
            .as_ref()
            .filter(|d| {
                requested
                    .as_ref()
                    .is_none_or(|name| name == &d.input_device)
            })
            .map_or(0, |d| d.input_channels);
        self.devices
            .inputs
            .iter()
            .find(|device| requested.as_ref().is_some_and(|name| name == &device.name))
            .map_or(observed.max(1), |device| {
                device.channels.max(observed).max(1)
            })
    }

    pub(super) fn mixer_levels(&self) -> [f32; 3] {
        self.engine
            .as_ref()
            .map_or([0.; 3], |engine| engine.mixer_levels())
    }

    pub(super) fn set_mix_gain(&mut self, role: TrackRole, gain: f32) {
        if !gain.is_finite() {
            return;
        }
        match role {
            TrackRole::Target => self.workspace.document.settings.target_gain = gain.clamp(0., 2.),
            TrackRole::Backing => {
                self.workspace.document.settings.backing_gain = gain.clamp(0., 2.)
            }
        }
        self.update_mix();
    }

    pub(super) fn set_voice_gain(&mut self, gain: f32) {
        if !gain.is_finite() {
            return;
        }
        self.workspace.document.settings.monitor_gain = gain.clamp(0., 2.);
        if let Some(engine) = &self.engine {
            engine.set_monitor_gain(gain.clamp(0., 2.));
        }
        self.changed();
    }

    pub(super) fn toggle_hear_myself(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.doc().settings.audio.monitor || self.monitor_requested {
            self.monitor_requested = false;
            self.workspace.document.settings.audio.monitor = false;
            if let Some(engine) = &self.engine {
                engine.set_monitor(false);
            }
        } else if self.input_running {
            self.workspace.document.settings.audio.monitor = true;
            if let Some(engine) = &self.engine {
                engine.set_monitor(true);
            }
        } else {
            self.monitor_requested = true;
            self.reconnect_microphone = true;
            if !self.input_pending && !self.permission_pending {
                self.toggle_input(window, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn choose_setup_audio(
        &mut self,
        role: TrackRole,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.native_dialog_open || self.busy.is_some() {
            return;
        }
        self.native_dialog_open = true;
        let receiver =
            platform::open_audio_dialog(window, &format!("Choose {}", role.label().to_lowercase()));
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.native_dialog_open = false;
                match result {
                    Ok(Ok(Some(path))) => match role {
                        TrackRole::Target => app.setup_reference = Some(path),
                        TrackRole::Backing => app.setup_accompaniment = Some(path),
                    },
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => app.fail(format!("Couldn't choose the recording: {error:#}")),
                    Err(_) => app.fail("The file sheet closed unexpectedly."),
                }
                app.focus.focus(window);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn clear_setup_audio(&mut self, role: TrackRole) {
        match role {
            TrackRole::Target => self.setup_reference = None,
            TrackRole::Backing => self.setup_accompaniment = None,
        }
    }

    pub(super) fn start_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.setup_reference.clone() {
            self.pending_backing = self.setup_accompaniment.clone();
            self.import_path(TrackRole::Target, path);
            self.focus.focus(window);
            cx.notify();
        }
    }

    pub(super) fn recording_active(&self) -> bool {
        self.recording
            .as_ref()
            .is_some_and(|recording| !recording.finalizing)
    }
    pub(super) fn recording_pending(&self) -> bool {
        self.record_requested
            || self
                .recording
                .as_ref()
                .is_some_and(|recording| recording.finalizing)
    }
    pub(super) fn recording_finalizing(&self) -> bool {
        self.recording
            .as_ref()
            .is_some_and(|recording| recording.finalizing)
    }

    pub(super) fn toggle_record(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recording_active() || self.record_requested {
            self.pause();
            return;
        }
        if self.recording.is_some()
            || self.busy.is_some()
            || self.doc().target.is_none()
            || self.replaying_take.is_some()
        {
            return;
        }
        self.pause();
        self.stop_take_playback();
        self.record_requested = true;
        self.record_started_microphone = !self.input_running
            && !self.input_pending
            && !self.permission_pending
            && !self.monitor_requested;
        self.recording_status = Some("Connecting microphone…".into());
        if !self.input_running && !self.input_pending && !self.permission_pending {
            self.reconnect_microphone = true;
            self.toggle_input(window, cx);
        }
        cx.notify();
    }

    pub(super) fn start_recorded_take(&mut self) {
        self.record_requested = false;
        self.record_started_microphone = false;
        if self.cursor >= self.doc().duration() - 0.01 {
            self.cursor = 0.;
        }
        let id = new_id();
        let path = match self.workspace.recording_destination(&id) {
            Ok(path) => path,
            Err(error) => {
                self.fail(format!("Couldn't prepare the recording: {error:#}"));
                return;
            }
        };
        let result = self
            .engine
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Audio is unavailable"))
            .and_then(|engine| engine.start_recording(path));
        match result {
            Ok(session) => {
                if let Some(engine) = &self.engine {
                    engine.set_loop(
                        self.doc().transport.loop_start,
                        self.doc().transport.loop_end,
                        false,
                    );
                }
                self.recording = Some(RecordingState {
                    session,
                    take: PracticeTake {
                        id: id.clone(),
                        started_at_ms: timestamp_ms(),
                        target_id: self
                            .doc()
                            .target
                            .as_ref()
                            .map_or(String::new(), |track| track.id.clone()),
                        start: self.cursor,
                        end: self.cursor,
                        reference_selection: Some(self.target.selection.clone()),
                        ..Default::default()
                    },
                    archive_path: format!("recordings/{id}.wav"),
                    finalizing: false,
                });
                match self
                    .engine
                    .as_ref()
                    .expect("Recording has an engine")
                    .play(self.cursor)
                {
                    Ok(()) => {
                        self.play_pending = true;
                        self.recording_status = Some("Recording your voice…".into());
                    }
                    Err(error) => {
                        self.stop_recording_capture();
                        self.fail(format!("Couldn't start playback: {error:#}"));
                    }
                }
            }
            Err(error) => self.fail(format!("Couldn't record your voice: {error:#}")),
        }
    }

    pub(super) fn stop_recording_capture(&mut self) {
        if self.record_requested && self.recording.is_none() {
            self.recording_status = Some("Recording canceled.".into());
            if self.record_started_microphone && !self.monitor_requested {
                self.microphone_requested = false;
                self.reconnect_microphone = false;
                self.input_pending = false;
                if let Some(engine) = &mut self.engine {
                    engine.stop_input();
                }
            }
        }
        self.record_requested = false;
        self.record_started_microphone = false;
        if let Some(recording) = &mut self.recording
            && !recording.finalizing
        {
            recording.take.end = self.cursor.max(recording.take.start);
            recording.session.stop();
            recording.finalizing = true;
            self.recording_status = Some("Saving the take…".into());
        }
    }

    pub(super) fn take_audio_available(&self, index: usize) -> bool {
        self.doc()
            .history
            .get(index)
            .and_then(|take| take.recording.as_ref())
            .is_some_and(|recording| {
                self.workspace
                    .recording_paths
                    .contains_key(&recording.archive_path)
            })
    }

    pub(super) fn play_take(&mut self, index: usize) {
        if self.recording.is_some() || self.record_requested {
            return;
        }
        let Some(take) = self.doc().history.get(index) else {
            return;
        };
        if let Some(problem) = comparison_problem(
            take,
            self.doc()
                .target
                .as_ref()
                .map(|reference| reference.id.as_str()),
            &self.target,
        ) {
            self.fail(problem);
            return;
        }
        let id = take.id.clone();
        if self.replaying_take.as_ref() == Some(&id) {
            self.stop_take_playback();
            return;
        }
        let path = take
            .recording
            .as_ref()
            .and_then(|recording| self.workspace.recording_paths.get(&recording.archive_path))
            .cloned();
        let Some(path) = path else {
            return;
        };
        self.pause();
        self.stop_take_playback();
        self.replay_restore = Some((self.cursor, self.view_start));
        self.workspace.document.settings.audio.monitor = false;
        self.monitor_requested = false;
        if let Some(engine) = &self.engine {
            engine.set_monitor(false);
            match engine.play_recording(path) {
                Ok(()) => self.replaying_take = Some(id),
                Err(error) => {
                    self.pause();
                    self.replay_restore = None;
                    self.fail(format!("Couldn't play the take: {error:#}"));
                }
            }
        }
    }

    pub(super) fn stop_take_playback(&mut self) {
        if self.replaying_take.take().is_some()
            && let Some(engine) = &self.engine
        {
            engine.stop_recording_playback();
        }
        if let Some((cursor, view)) = self.replay_restore.take() {
            self.cursor = cursor;
            self.view_start = view;
        }
    }

    pub(super) fn tick_session(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.preference_results.try_recv() {
            self.preferences_status = Some(match result {
                Ok(()) => "Audio devices saved on this Mac".into(),
                Err(error) => error,
            });
            changed = true;
        }
        if self.reconnect_microphone
            && !self.audio_starting
            && !self.audio_configuring
            && !self.input_pending
            && !self.permission_pending
        {
            self.reconnect_microphone = false;
            if self.microphone_authorization == platform::MicrophoneAuthorization::Authorized
                && !self.input_running
            {
                self.start_authorized_input();
            }
        }
        if self.record_requested
            && self.input_running
            && !self.audio_starting
            && !self.audio_configuring
        {
            self.start_recorded_take();
            changed = true;
        }
        if self.recording_active() && !self.playing() && !self.play_pending {
            self.finish_take();
            self.stop_recording_capture();
        }
        if let Some(result) = self
            .recording
            .as_ref()
            .and_then(|recording| recording.session.try_finish())
        {
            self.finish_take();
            let mut recording = self.recording.take().expect("A completed recording exists");
            if let Some(engine) = &self.engine {
                engine.set_loop(
                    self.doc().transport.loop_start,
                    self.doc().transport.loop_end,
                    self.doc().transport.loop_enabled,
                );
            }
            match result {
                Ok(audio) if audio.frames > 0 => {
                    recording.take.recording = Some(TakeRecording {
                        archive_path: recording.archive_path.clone(),
                        sample_rate: audio.sample_rate,
                        frames: audio.frames,
                    });
                    self.workspace
                        .recording_paths
                        .insert(recording.archive_path, audio.path);
                    self.workspace.document.history.push(recording.take);
                    self.recording_status = Some("Take saved. Select Listen to review it.".into());
                    self.changed();
                }
                Ok(_) => self.recording_status = Some("No microphone audio was captured.".into()),
                Err(error) => {
                    self.recording_status =
                        Some("Recording failed. The take was not saved.".into());
                    self.pause();
                    self.save_after_recording = None;
                    self.deferred_document_intent = None;
                    self.lifecycle.cancel();
                    self.fail(format!("Couldn't save the recording: {error:#}"));
                }
            }
            changed = true;
        }
        if self.replaying_take.is_some() {
            let replay = self
                .engine
                .as_ref()
                .map(|engine| engine.recording_playback_state());
            if replay.is_none_or(|replay| !replay.playing && !replay.pending) {
                self.stop_take_playback();
                changed = true;
            } else if let Some(replay) = replay
                && replay.playing
                && let Some(take) = self
                    .doc()
                    .history
                    .iter()
                    .find(|take| self.replaying_take.as_ref() == Some(&take.id))
            {
                self.cursor = (take.start + replay.position).min(take.end);
                let seconds = self.doc().transport.visible_seconds;
                if self.cursor > self.view_start + seconds * 0.8 || self.cursor < self.view_start {
                    self.view_start = (self.cursor - seconds * 0.2).max(0.);
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replaced_recording_cannot_be_compared_to_a_previous_take() {
        let take = PracticeTake {
            target_id: "first-recording".into(),
            ..Default::default()
        };
        assert!(
            comparison_problem(&take, Some("second-recording"), &TrackAnalysis::default())
                .is_some()
        );
        assert!(
            comparison_problem(&take, Some("first-recording"), &TrackAnalysis::default()).is_none()
        );
    }

    #[test]
    fn polyphonic_take_comparison_requires_its_scored_line_and_register() {
        let mut reference = TrackAnalysis {
            mode: AnalysisMode::Polyphonic,
            ..Default::default()
        };
        let mut take = PracticeTake {
            target_id: "reference".into(),
            evaluated_seconds: 4.,
            reference_selection: Some(reference.selection.clone()),
            ..Default::default()
        };
        assert!(comparison_problem(&take, Some("reference"), &reference).is_none());
        reference.selection.policy = LinePolicy::Lowest;
        assert!(comparison_problem(&take, Some("reference"), &reference).is_some());
        reference.selection.policy = LinePolicy::Highest;
        reference.selection.min_midi += 1;
        assert!(comparison_problem(&take, Some("reference"), &reference).is_some());
        // An unscored audio take can still be heard without claiming a comparison.
        take.evaluated_seconds = 0.;
        assert!(comparison_problem(&take, Some("reference"), &reference).is_none());
    }
}
