//! UI-facing audio actor. All hardware calls and stream ownership stay on its dedicated thread.
use crate::{
    audio::{
        self, AudioDiagnostics, AudioEngine, AudioHandle, CaptureEpoch, DeviceInventory,
        ReplaySnapshot,
    },
    model::{AudioPreferences, PitchFrame},
    recording::RecordingSession,
};
use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, Sender, bounded};
use crossbeam_queue::ArrayQueue;
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Clone, Debug, Default)]
pub struct ControllerSnapshot {
    pub initializing: bool,
    pub configuring: bool,
    pub error: Option<String>,
    pub error_seq: u64,
    pub command_seq: u64,
    pub completed_seq: u64,
    pub diagnostics: AudioDiagnostics,
    pub devices: DeviceInventory,
    pub replay: ReplaySnapshot,
}
struct Shared {
    snapshot: Mutex<ControllerSnapshot>,
    handle: Mutex<Option<AudioHandle>>,
    pitch: ArrayQueue<(PitchFrame, CaptureEpoch)>,
    shutdown: AtomicBool,
    next_command: AtomicU64,
    latest_tracks: AtomicU64,
    latest_configure: AtomicU64,
    latest_input: AtomicU64,
    latest_monitor: AtomicU64,
    latest_gains: AtomicU64,
    latest_monitor_gain: AtomicU64,
    latest_replay: AtomicU64,
    replay_pending: AtomicBool,
}
enum Operation {
    Tracks(Option<PathBuf>, Option<PathBuf>),
    Play(f64),
    Pause,
    Seek(f64),
    Loop(f64, f64, bool),
    Gains(f32, f32),
    MonitorGain(f32),
    PlayRecording(PathBuf),
    StopRecordingPlayback,
    Configure(AudioPreferences),
    StartInput(AudioPreferences),
    StopInput,
    Monitor(bool),
    Compensation(f32),
    RefreshDevices,
    Shutdown,
}
struct Request {
    sequence: u64,
    operation: Operation,
}

pub struct AudioController {
    shared: Arc<Shared>,
    sender: Sender<Request>,
    thread: Option<JoinHandle<()>>,
    cached: RefCell<ControllerSnapshot>,
}
impl AudioController {
    /// Returns immediately, before any audio host or device API is called.
    pub fn new() -> Result<Self> {
        Self::new_with_preferences(&AudioPreferences::default())
    }
    pub fn new_with_preferences(preferences: &AudioPreferences) -> Result<Self> {
        let preferences = preferences.clone();
        Self::spawn(move |receive, shared| owner(receive, shared, preferences))
    }
    fn spawn(run: impl FnOnce(Receiver<Request>, Arc<Shared>) + Send + 'static) -> Result<Self> {
        let snapshot = ControllerSnapshot {
            initializing: true,
            ..Default::default()
        };
        let shared = Arc::new(Shared {
            snapshot: Mutex::new(snapshot.clone()),
            handle: Mutex::new(None),
            pitch: ArrayQueue::new(1024),
            shutdown: AtomicBool::new(false),
            next_command: AtomicU64::new(0),
            latest_tracks: AtomicU64::new(0),
            latest_configure: AtomicU64::new(0),
            latest_input: AtomicU64::new(0),
            latest_monitor: AtomicU64::new(0),
            latest_gains: AtomicU64::new(0),
            latest_monitor_gain: AtomicU64::new(0),
            latest_replay: AtomicU64::new(0),
            replay_pending: AtomicBool::new(false),
        });
        let (sender, receive) = bounded(64);
        let worker_shared = shared.clone();
        let thread = thread::Builder::new()
            .name("voice-audio-owner".into())
            .spawn(move || run(receive, worker_shared))
            .context("Cannot start the audio owner thread")?;
        Ok(Self {
            shared,
            sender,
            thread: Some(thread),
            cached: RefCell::new(snapshot),
        })
    }
    fn send(&self, operation: Operation) -> Result<()> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            bail!("The audio controller is shutting down");
        }
        let sequence = self.shared.next_command.fetch_add(1, Ordering::AcqRel) + 1;
        let slot = request_slot(&self.shared, &operation);
        let previous = slot.map(|slot| slot.swap(sequence, Ordering::AcqRel));
        if let Err(error) = self.sender.try_send(Request {
            sequence,
            operation,
        }) {
            if let (Some(slot), Some(previous)) = (slot, previous) {
                let _ =
                    slot.compare_exchange(sequence, previous, Ordering::AcqRel, Ordering::Acquire);
            }
            return Err(error).context("The audio command queue is busy or unavailable");
        }
        if let Ok(mut state) = self.shared.snapshot.try_lock() {
            state.command_seq = sequence;
        }
        Ok(())
    }
    fn handle(&self) -> Option<AudioHandle> {
        self.shared
            .handle
            .try_lock()
            .ok()
            .and_then(|handle| handle.clone())
    }
    pub fn poll(&self) -> ControllerSnapshot {
        if let Ok(snapshot) = self.shared.snapshot.try_lock() {
            *self.cached.borrow_mut() = snapshot.clone();
        }
        let mut snapshot = self.cached.borrow_mut();
        if let Some(handle) = self.handle() {
            handle.refresh_diagnostics(&mut snapshot.diagnostics);
            snapshot.replay = handle.recording_playback_state();
        }
        snapshot.replay.pending |= self.shared.replay_pending.load(Ordering::Acquire);
        snapshot.clone()
    }
    pub fn set_tracks(&mut self, target: Option<PathBuf>, backing: Option<PathBuf>) -> Result<()> {
        self.pause();
        self.send(Operation::Tracks(target, backing))
    }
    pub fn play(&self, offset: f64) -> Result<()> {
        let state = self.poll();
        if state.initializing || state.configuring {
            bail!("Audio is still initializing or configuring");
        }
        self.send(Operation::Play(offset))
    }
    pub fn pause(&self) -> f64 {
        let position = self.handle().map_or(0.0, |handle| handle.pause());
        let _ = self.send(Operation::Pause);
        position
    }
    pub fn seek(&self, seconds: f64) -> Result<()> {
        self.send(Operation::Seek(seconds))
    }
    pub fn position(&self) -> f64 {
        self.handle().map_or(0.0, |handle| handle.position())
    }
    pub fn is_playing(&self) -> bool {
        self.handle().is_some_and(|handle| handle.is_playing())
    }
    pub fn is_input_running(&self) -> bool {
        self.handle()
            .is_some_and(|handle| handle.is_input_running())
    }
    pub fn loop_iteration(&self) -> u64 {
        self.handle().map_or(0, |handle| handle.loop_iteration())
    }
    pub fn set_loop(&self, start: f64, end: f64, enabled: bool) {
        let _ = self.send(Operation::Loop(start, end, enabled));
    }
    pub fn set_gains(&self, target: f32, backing: f32) {
        if let Some(handle) = self.handle() {
            self.shared.latest_gains.store(u64::MAX, Ordering::Release);
            handle.set_gains(target, backing);
        } else {
            let _ = self.send(Operation::Gains(target, backing));
        }
    }
    pub fn set_monitor_gain(&self, gain: f32) {
        if let Some(handle) = self.handle() {
            self.shared
                .latest_monitor_gain
                .store(u64::MAX, Ordering::Release);
            handle.set_monitor_gain(gain);
        } else {
            let _ = self.send(Operation::MonitorGain(gain));
        }
    }
    /// Returns peak levels since the last read: voice input, reference, accompaniment.
    pub fn mixer_levels(&self) -> [f32; 3] {
        self.handle()
            .map_or([0.0; 3], |handle| handle.mixer_levels())
    }
    pub fn start_recording(&self, path: PathBuf) -> Result<RecordingSession> {
        let state = self.poll();
        if state.initializing || state.configuring {
            bail!("Audio is still initializing or configuring");
        }
        self.handle()
            .context("Audio is still initializing")?
            .start_recording(path)
    }
    pub fn recording_playback_state(&self) -> ReplaySnapshot {
        self.poll().replay
    }
    pub fn play_recording(&self, path: PathBuf) -> Result<()> {
        let state = self.poll();
        if state.initializing || state.configuring {
            bail!("Audio is still initializing or configuring");
        }
        self.pause();
        if let Some(handle) = self.handle() {
            handle.stop_recording_playback();
        }
        self.shared.replay_pending.store(true, Ordering::Release);
        if let Err(error) = self.send(Operation::PlayRecording(path)) {
            self.shared.replay_pending.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(())
    }
    pub fn stop_recording_playback(&self) {
        self.shared.replay_pending.store(false, Ordering::Release);
        // Cancellation remains authoritative even if the bounded command queue is full.
        self.shared.latest_replay.store(u64::MAX, Ordering::Release);
        if let Some(handle) = self.handle() {
            handle.stop_recording_playback();
        }
        let _ = self.send(Operation::StopRecordingPlayback);
    }
    pub fn configure_audio(&mut self, preferences: &AudioPreferences) -> Result<()> {
        self.pause();
        self.send(Operation::Configure(preferences.clone()))
    }
    pub fn start_input(&mut self, preferences: &AudioPreferences) -> Result<()> {
        let state = self.poll();
        if state.initializing || state.configuring {
            bail!("Audio is still initializing or configuring");
        }
        self.send(Operation::StartInput(preferences.clone()))
    }
    pub fn stop_input(&mut self) {
        self.shared.latest_input.store(u64::MAX, Ordering::Release);
        if let Some(handle) = self.handle() {
            handle.stop_input_capture();
        }
        let _ = self.send(Operation::StopInput);
    }
    pub fn set_monitor(&self, enabled: bool) {
        if !enabled {
            self.shared
                .latest_monitor
                .store(u64::MAX, Ordering::Release);
        }
        if let Some(handle) = self.handle() {
            handle.set_monitor(enabled);
        }
        let _ = self.send(Operation::Monitor(enabled));
    }
    pub fn set_latency_compensation(&self, milliseconds: f32) {
        if let Some(handle) = self.handle() {
            handle.set_latency_compensation(milliseconds);
        } else {
            let _ = self.send(Operation::Compensation(milliseconds));
        }
    }
    pub fn refresh_devices(&self) -> Result<()> {
        self.send(Operation::RefreshDevices)
    }
    pub fn drain_pitch(&self) -> Vec<PitchFrame> {
        let epoch = self.handle().map(|handle| handle.capture_epoch());
        let mut result = Vec::new();
        while let Some((frame, captured_epoch)) = self.shared.pitch.pop() {
            if Some(captured_epoch) == epoch {
                result.push(frame);
            }
        }
        result
    }
    pub fn diagnostics(&self) -> AudioDiagnostics {
        self.poll().diagnostics
    }
}
impl Drop for AudioController {
    fn drop(&mut self) {
        if let Some(handle) = self.handle() {
            handle.shutdown();
        }
        self.shared.shutdown.store(true, Ordering::Release);
        let _ = self.sender.try_send(Request {
            sequence: 0,
            operation: Operation::Shutdown,
        });
        // A native driver call cannot be canceled safely. Never join it on the GPUI thread.
        drop(self.thread.take());
    }
}
fn update(shared: &Shared, operation: impl FnOnce(&mut ControllerSnapshot)) {
    if let Ok(mut state) = shared.snapshot.lock() {
        operation(&mut state);
    }
}
fn record_error(shared: &Shared, error: anyhow::Error) {
    update(shared, |state| {
        state.error_seq += 1;
        state.error = Some(format!("{error:#}"));
    });
}
fn configure(shared: &Shared, operation: impl FnOnce() -> Result<()>) -> Result<()> {
    update(shared, |state| state.configuring = true);
    let result = operation();
    update(shared, |state| state.configuring = false);
    result
}
fn update_diagnostics(shared: &Shared, engine: &AudioEngine) {
    let diagnostics = engine.diagnostics();
    update(shared, |state| {
        for device in &mut state.devices.outputs {
            if device.name == diagnostics.output_device {
                device.channels = diagnostics.output_channels;
                device.sample_rate_min = diagnostics.sample_rate;
                device.sample_rate_max = diagnostics.sample_rate;
            }
        }
        for device in &mut state.devices.inputs {
            if device.name == diagnostics.input_device {
                device.channels = diagnostics.input_channels;
                device.sample_rate_min = diagnostics.input_sample_rate;
                device.sample_rate_max = diagnostics.input_sample_rate;
            }
        }
        state.diagnostics = diagnostics;
    });
}
fn request_slot<'a>(shared: &'a Shared, operation: &Operation) -> Option<&'a AtomicU64> {
    match operation {
        Operation::Tracks(_, _) => Some(&shared.latest_tracks),
        Operation::Configure(_) => Some(&shared.latest_configure),
        Operation::StartInput(_) | Operation::StopInput => Some(&shared.latest_input),
        Operation::Monitor(_) => Some(&shared.latest_monitor),
        Operation::Gains(_, _) => Some(&shared.latest_gains),
        Operation::MonitorGain(_) => Some(&shared.latest_monitor_gain),
        Operation::PlayRecording(_) | Operation::StopRecordingPlayback => {
            Some(&shared.latest_replay)
        }
        _ => None,
    }
}
fn request_is_current(shared: &Shared, request: &Request) -> bool {
    request_slot(shared, &request.operation)
        .is_none_or(|slot| slot.load(Ordering::Acquire) == request.sequence)
}

fn owner(receive: Receiver<Request>, shared: Arc<Shared>, preferences: AudioPreferences) {
    // Inventory uses CPAL 0.18 device properties and never opens an AudioUnit.
    match audio::list_device_names() {
        Ok(devices) => update(&shared, |state| state.devices = devices),
        Err(error) => record_error(&shared, error.context("Cannot enumerate audio devices")),
    }
    if shared.shutdown.load(Ordering::Acquire) {
        return;
    }
    let mut engine = match AudioEngine::unconfigured() {
        Ok(engine) => engine,
        Err(error) => {
            record_error(&shared, error);
            update(&shared, |state| state.initializing = false);
            return;
        }
    };
    if let Ok(mut handle) = shared.handle.lock() {
        *handle = Some(engine.handle());
    }
    if shared.shutdown.load(Ordering::Acquire) {
        return;
    }
    if let Err(error) = configure(&shared, || engine.configure_audio(&preferences)) {
        record_error(
            &shared,
            error.context("Cannot initialize the default output"),
        );
    }
    update(&shared, |state| state.initializing = false);
    update_diagnostics(&shared, &engine);
    while !shared.shutdown.load(Ordering::Acquire) {
        match receive.recv_timeout(Duration::from_millis(10)) {
            Ok(request) => {
                if !request_is_current(&shared, &request) {
                    update(&shared, |state| state.completed_seq = request.sequence);
                    continue;
                }
                let slot = request_slot(&shared, &request.operation);
                let starting_input = matches!(&request.operation, Operation::StartInput(_));
                let starting_replay = matches!(&request.operation, Operation::PlayRecording(_));
                let result = match request.operation {
                    Operation::Tracks(target, backing) => engine.set_tracks(target, backing),
                    Operation::Play(offset) => engine.play(offset),
                    Operation::Pause => {
                        engine.pause();
                        Ok(())
                    }
                    Operation::Seek(seconds) => engine.seek(seconds),
                    Operation::Loop(start, end, enabled) => {
                        engine.set_loop(start, end, enabled);
                        Ok(())
                    }
                    Operation::Gains(target, backing) => {
                        engine.set_gains(target, backing);
                        Ok(())
                    }
                    Operation::MonitorGain(gain) => {
                        engine.set_monitor_gain(gain);
                        Ok(())
                    }
                    Operation::PlayRecording(path) => engine.play_recording(path),
                    Operation::StopRecordingPlayback => {
                        engine.stop_recording_playback();
                        Ok(())
                    }
                    Operation::Configure(preferences) => {
                        configure(&shared, || engine.configure_audio(&preferences))
                    }
                    Operation::StartInput(preferences) => {
                        configure(&shared, || engine.start_input(&preferences))
                    }
                    Operation::StopInput => configure(&shared, || {
                        engine.stop_input();
                        Ok(())
                    }),
                    Operation::Monitor(enabled) => {
                        engine.set_monitor(enabled);
                        Ok(())
                    }
                    Operation::Compensation(milliseconds) => {
                        engine.set_latency_compensation(milliseconds);
                        Ok(())
                    }
                    Operation::RefreshDevices => match audio::list_device_names() {
                        Ok(devices) => {
                            update(&shared, |state| state.devices = devices);
                            Ok(())
                        }
                        Err(error) => Err(error),
                    },
                    Operation::Shutdown => break,
                };
                let current =
                    slot.is_none_or(|slot| slot.load(Ordering::Acquire) == request.sequence);
                if !current && starting_input {
                    engine.stop_input();
                }
                if !current && starting_replay {
                    engine.stop_recording_playback();
                }
                if current && starting_replay {
                    shared.replay_pending.store(false, Ordering::Release);
                }
                if current && let Err(error) = result {
                    record_error(&shared, error);
                }
                update(&shared, |state| state.completed_seq = request.sequence);
                update_diagnostics(&shared, &engine);
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
        for frame in engine.handle().drain_capture() {
            if shared.pitch.push(frame).is_err() {
                let _ = shared.pitch.pop();
            }
        }
        if engine.handle().take_replay_error() {
            record_error(
                &shared,
                anyhow::anyhow!("Take playback stopped because the recording could not be read"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    #[test]
    fn canceled_queued_take_playback_never_reaches_the_owner() {
        let (release, blocked) = bounded::<()>(1);
        let (ready, initialized) = bounded(1);
        let (finished, result) = bounded(1);
        let controller = AudioController::spawn(move |receive, shared| {
            update(&shared, |state| state.initializing = false);
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            let operations: Vec<_> = receive
                .try_iter()
                .filter(|r| request_is_current(&shared, r))
                .map(|r| match r.operation {
                    Operation::PlayRecording(_) => "play",
                    Operation::StopRecordingPlayback => "stop",
                    _ => "other",
                })
                .collect();
            finished.send(operations).unwrap();
        })
        .unwrap();
        initialized.recv_timeout(Duration::from_secs(1)).unwrap();
        controller
            .play_recording(PathBuf::from("take.wav"))
            .unwrap();
        assert!(controller.recording_playback_state().pending);
        controller.stop_recording_playback();
        assert!(!controller.recording_playback_state().pending);
        release.send(()).unwrap();
        let operations = result.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(!operations.contains(&"play"));
        assert!(operations.contains(&"stop"));
    }
    #[test]
    fn cancellation_remains_authoritative_when_the_owner_queue_is_full() {
        let (release, blocked) = bounded::<()>(1);
        let (finished, result) = bounded(1);
        let mut controller = AudioController::spawn(move |receive, shared| {
            blocked.recv().unwrap();
            let openings = receive
                .try_iter()
                .filter(|r| request_is_current(&shared, r))
                .filter(|r| {
                    matches!(
                        r.operation,
                        Operation::StartInput(_)
                            | Operation::Monitor(true)
                            | Operation::PlayRecording(_)
                    )
                })
                .count();
            finished.send(openings).unwrap();
        })
        .unwrap();
        controller
            .send(Operation::StartInput(AudioPreferences::default()))
            .unwrap();
        controller.send(Operation::Monitor(true)).unwrap();
        controller
            .send(Operation::PlayRecording(PathBuf::from("take.wav")))
            .unwrap();
        for _ in 0..61 {
            controller.refresh_devices().unwrap();
        }
        controller.stop_input();
        controller.set_monitor(false);
        controller.stop_recording_playback();
        release.send(()).unwrap();
        assert_eq!(result.recv_timeout(Duration::from_secs(1)).unwrap(), 0);
    }
    #[test]
    fn direct_faders_supersede_queued_startup_values_and_bypass_a_full_owner_queue() {
        let (activate, activation) = bounded::<()>(1);
        let (release, blocked) = bounded::<()>(1);
        let (ready, initialized) = bounded(1);
        let (finished, result) = bounded(1);
        let controller = AudioController::spawn(move |receive, shared| {
            activation.recv().unwrap();
            let engine = AudioEngine::unconfigured().unwrap();
            *shared.handle.lock().unwrap() = Some(engine.handle());
            ready.send(()).unwrap();
            blocked.recv().unwrap();
            let obsolete_faders = receive
                .try_iter()
                .filter(|r| request_is_current(&shared, r))
                .filter(|r| {
                    matches!(
                        r.operation,
                        Operation::Gains(_, _) | Operation::MonitorGain(_)
                    )
                })
                .count();
            finished.send(obsolete_faders).unwrap();
        })
        .unwrap();
        controller.set_gains(0.1, 0.2);
        controller.set_monitor_gain(0.1);
        activate.send(()).unwrap();
        initialized.recv_timeout(Duration::from_secs(1)).unwrap();
        for _ in 0..62 {
            controller.refresh_devices().unwrap();
        }
        assert!(controller.refresh_devices().is_err());
        let sequence = controller.shared.next_command.load(Ordering::Acquire);
        controller.set_gains(0.9, 0.8);
        controller.set_monitor_gain(0.75);
        assert_eq!(
            controller.shared.next_command.load(Ordering::Acquire),
            sequence
        );
        release.send(()).unwrap();
        assert_eq!(result.recv_timeout(Duration::from_secs(1)).unwrap(), 0);
    }
    #[test]
    fn stalled_owner_never_blocks_construction_poll_pause_or_drop() {
        let (release, blocked) = bounded::<()>(1);
        let (finished, finish_receive) = bounded(1);
        let started = Instant::now();
        let controller = AudioController::spawn(move |_, shared| {
            update(&shared, |state| state.configuring = true);
            let _ = blocked.recv();
            let _ = finished.send(shared.shutdown.load(Ordering::Acquire));
        })
        .unwrap();
        assert!(controller.poll().initializing);
        controller.pause();
        drop(controller);
        assert!(started.elapsed() < Duration::from_millis(250));
        release.send(()).unwrap();
        assert!(finish_receive.recv_timeout(Duration::from_secs(1)).unwrap());
    }
    #[test]
    fn command_overflow_is_bounded_and_reports_backpressure() {
        let (release, blocked) = bounded::<()>(1);
        let controller = AudioController::spawn(move |receive, _| {
            let _keep_receive = receive;
            let _ = blocked.recv();
        })
        .unwrap();
        for _ in 0..64 {
            controller.refresh_devices().unwrap();
        }
        assert!(controller.refresh_devices().is_err());
        drop(controller);
        release.send(()).unwrap();
    }
    #[test]
    fn error_sequences_change_only_for_new_failures() {
        let (release, blocked) = bounded::<()>(1);
        let controller = AudioController::spawn(move |receive, _| {
            let _keep_receive = receive;
            let _ = blocked.recv();
        })
        .unwrap();
        record_error(&controller.shared, anyhow::anyhow!("Device unavailable"));
        let first = controller.poll();
        let second = controller.poll();
        assert_eq!(first.error_seq, second.error_seq);
        assert_eq!(first.error.as_deref(), Some("Device unavailable"));
        record_error(&controller.shared, anyhow::anyhow!("Different failure"));
        assert_eq!(controller.poll().error_seq, first.error_seq + 1);
        drop(controller);
        release.send(()).unwrap();
    }
    #[test]
    fn pitch_drain_rejects_frames_queued_before_a_transport_epoch_change() {
        let (release, blocked) = bounded::<()>(1);
        let (ready, initialized) = bounded(1);
        let (finished, complete) = bounded(1);
        let controller = AudioController::spawn(move |receive, shared| {
            let engine = AudioEngine::unconfigured().unwrap();
            *shared.handle.lock().unwrap() = Some(engine.handle());
            let _keep_receive = receive;
            ready.send(()).unwrap();
            let _ = blocked.recv();
            drop(engine);
            let _ = finished.send(());
        })
        .unwrap();
        initialized.recv_timeout(Duration::from_secs(1)).unwrap();
        let handle = controller.handle().unwrap();
        let old_epoch = handle.capture_epoch();
        controller
            .shared
            .pitch
            .push((
                PitchFrame {
                    midi: 60.0,
                    ..Default::default()
                },
                old_epoch,
            ))
            .ok();
        handle.pause();
        controller
            .shared
            .pitch
            .push((
                PitchFrame {
                    midi: 69.0,
                    ..Default::default()
                },
                handle.capture_epoch(),
            ))
            .ok();
        let frames = controller.drain_pitch();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].midi, 69.0);
        drop(controller);
        release.send(()).unwrap();
        complete.recv_timeout(Duration::from_secs(1)).unwrap();
    }
    #[test]
    fn queued_tracks_configuration_and_input_open_are_superseded_by_newer_requests() {
        let (release, blocked) = bounded::<()>(1);
        let (finished, result) = bounded(1);
        let mut controller = AudioController::spawn(move |receive, shared| {
            let _ = blocked.recv();
            let current: Vec<_> = receive
                .try_iter()
                .filter(|request| request_is_current(&shared, request))
                .map(|request| match request.operation {
                    Operation::Tracks(target, _) => target.unwrap().to_string_lossy().into_owned(),
                    Operation::Configure(settings) => format!("buffer:{}", settings.buffer_frames),
                    Operation::StartInput(_) => "start".into(),
                    Operation::StopInput => "stop".into(),
                    _ => "other".into(),
                })
                .collect();
            finished.send(current).unwrap();
        })
        .unwrap();
        controller
            .set_tracks(Some(PathBuf::from("old-project.wav")), None)
            .unwrap();
        controller
            .configure_audio(&AudioPreferences::default())
            .unwrap();
        controller
            .send(Operation::StartInput(AudioPreferences::default()))
            .unwrap();
        controller.stop_input();
        controller
            .set_tracks(Some(PathBuf::from("new-project.wav")), None)
            .unwrap();
        controller
            .configure_audio(&AudioPreferences {
                buffer_frames: 512,
                ..Default::default()
            })
            .unwrap();
        release.send(()).unwrap();
        let active = result.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            !active.iter().any(|label| label == "old-project.wav"
                || label == "buffer:256"
                || label == "start")
        );
        assert!(active.iter().any(|label| label == "new-project.wav"));
        assert!(active.iter().any(|label| label == "buffer:512"));
        assert!(active.iter().any(|label| label == "stop"));
    }
    #[test]
    fn rejected_request_does_not_supersede_an_accepted_request() {
        let (release, blocked) = bounded::<()>(1);
        let controller = AudioController::spawn(move |receive, _| {
            let _keep_receive = receive;
            let _ = blocked.recv();
        })
        .unwrap();
        for _ in 0..64 {
            controller.refresh_devices().unwrap();
        }
        assert!(
            controller
                .send(Operation::Tracks(
                    Some(PathBuf::from("not-accepted.wav")),
                    None
                ))
                .is_err()
        );
        assert_eq!(controller.shared.latest_tracks.load(Ordering::Acquire), 0);
        drop(controller);
        release.send(()).unwrap();
    }
    #[test]
    #[ignore = "Manual output-only hardware diagnostic; never runs in the core test suite"]
    fn native_controller_output_probe() {
        let controller = AudioController::new().unwrap();
        let mut observed_output = false;
        for second in 0..10 {
            thread::sleep(Duration::from_secs(1));
            let state = controller.poll();
            observed_output |= state.diagnostics.output_buffer_frames > 0;
            eprintln!(
                "probe second={} initializing={} configuring={} output={:?} observed_frames={} devices={:?} error={:?}",
                second + 1,
                state.initializing,
                state.configuring,
                state.diagnostics.output_device,
                state.diagnostics.output_buffer_frames,
                state
                    .devices
                    .outputs
                    .iter()
                    .map(|d| &d.name)
                    .collect::<Vec<_>>(),
                state.error
            );
        }
        drop(controller);
        eprintln!("probe controller dropped without joining hardware owner");
        assert!(
            observed_output,
            "No output callback was observed within 10 seconds"
        );
    }
    #[test]
    fn diagnostics_follow_live_callbacks_without_another_owner_command() {
        let (release, blocked) = bounded::<()>(1);
        let (ready, initialized) = bounded(1);
        let (finished, complete) = bounded(1);
        let controller = AudioController::spawn(move |receive, shared| {
            let engine = AudioEngine::unconfigured().unwrap();
            *shared.handle.lock().unwrap() = Some(engine.handle());
            update_diagnostics(&shared, &engine);
            let _keep_receive = receive;
            ready.send(()).unwrap();
            let _ = blocked.recv();
            drop(engine);
            let _ = finished.send(());
        })
        .unwrap();
        initialized.recv_timeout(Duration::from_secs(1)).unwrap();
        let before = controller.poll();
        assert_eq!(before.diagnostics.output_buffer_frames, 0);
        let handle = controller.handle().unwrap();
        crate::audio::output_callback_for_test(&handle, 256, true);
        let first = controller.poll();
        assert_eq!(first.diagnostics.output_buffer_frames, 256);
        assert_eq!(
            first.diagnostics.underruns,
            before.diagnostics.underruns + 256
        );
        assert_eq!(first.command_seq, before.command_seq);
        assert_eq!(first.completed_seq, before.completed_seq);
        crate::audio::output_callback_for_test(&handle, 64, false);
        crate::audio::output_error_for_test(&handle);
        let second = controller.poll();
        assert_eq!(second.diagnostics.output_buffer_frames, 64);
        assert_eq!(second.diagnostics.underruns, first.diagnostics.underruns);
        assert!(second.diagnostics.stream_error);
        assert_eq!(second.completed_seq, before.completed_seq);
        drop(controller);
        release.send(()).unwrap();
        complete.recv_timeout(Duration::from_secs(1)).unwrap();
    }
}
