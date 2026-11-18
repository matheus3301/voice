//! Native audio I/O. Callbacks only move bounded, generation-tagged samples and update atomics.
use crate::{
    analysis::detect_pitch,
    media::AudioReader,
    model::{AudioPreferences, PitchFrame},
    recording::{RecordingBus, RecordingReader, RecordingSession},
};
use anyhow::{Context, Result, bail};
use cpal::{
    BufferSize, SampleFormat, StreamConfig, SupportedBufferSize,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crossbeam_channel::{Receiver, Sender, bounded};
use crossbeam_queue::ArrayQueue;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const PLAYBACK_CAPACITY: usize = 8192;
const INPUT_CAPACITY: usize = 16384;
const PITCH_CAPACITY: usize = 512;
fn load_f64(value: &AtomicU64) -> f64 {
    f64::from_bits(value.load(Ordering::Acquire))
}
fn store_f64(value: &AtomicU64, number: f64) {
    value.store(number.to_bits(), Ordering::Release);
}
fn safe_output(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}
fn load_f32(value: &AtomicU32) -> f32 {
    f32::from_bits(value.load(Ordering::Relaxed))
}

#[derive(Clone, Debug, Default)]
pub struct DeviceInfo {
    pub name: String,
    pub channels: u16,
    pub sample_rate_min: u32,
    pub sample_rate_max: u32,
    pub buffer_min: Option<u32>,
    pub buffer_max: Option<u32>,
}
#[derive(Clone, Debug, Default)]
pub struct DeviceInventory {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}
#[derive(Clone, Debug, Default)]
pub struct AudioDiagnostics {
    pub output_device: String,
    pub input_device: String,
    pub sample_rate: u32,
    pub input_sample_rate: u32,
    pub input_channels: u16,
    pub output_channels: u16,
    pub requested_buffer_frames: u32,
    pub output_buffer_frames: u32,
    pub input_buffer_frames: u32,
    pub buffer_fallback: bool,
    pub underruns: u64,
    pub input_drops: u64,
    pub stream_error: bool,
    pub analysis_window_frames: u32,
    pub analysis_delay_seconds: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReplaySnapshot {
    pub playing: bool,
    pub pending: bool,
    pub position: f64,
    pub duration: f64,
}

#[derive(Clone, Copy)]
struct ReplayFrame {
    generation: u64,
    position: f64,
    value: f32,
    ended: bool,
}

#[derive(Clone, Copy, Default)]
struct MixFrame {
    generation: u64,
    position: f64,
    target: [f32; 2],
    backing: [f32; 2],
    ended: bool,
    iteration: u64,
}
#[derive(Clone, Copy)]
struct InputFrame {
    value: f32,
    time: f64,
    generation: u64,
    iteration: u64,
    input_generation: u64,
    playing: bool,
}
struct TaggedPitch {
    frame: PitchFrame,
    generation: u64,
    iteration: u64,
    input_generation: u64,
}
struct Shared {
    playback: ArrayQueue<MixFrame>,
    input: ArrayQueue<InputFrame>,
    monitor: ArrayQueue<f32>,
    pitch: ArrayQueue<TaggedPitch>,
    recording: Arc<RecordingBus>,
    replay: ArrayQueue<ReplayFrame>,
    replay_active: AtomicBool,
    replay_pending: AtomicBool,
    replay_generation: AtomicU64,
    replay_position: AtomicU64,
    replay_duration: AtomicU64,
    replay_error: AtomicBool,
    monitor_gain: AtomicU32,
    voice_peak: AtomicU32,
    target_peak: AtomicU32,
    backing_peak: AtomicU32,
    origin: Instant,
    callback_at: AtomicU64,
    output_lead: AtomicU64,
    input_generation: AtomicU64,
    transport_epoch_at: AtomicU64,
    input_epoch_at: AtomicU64,
    output_active: AtomicBool,
    playing: AtomicBool,
    shutdown: AtomicBool,
    input_active: AtomicBool,
    monitor_enabled: AtomicBool,
    generation: AtomicU64,
    monitor_generation: AtomicU64,
    position: AtomicU64,
    iteration: AtomicU64,
    target_gain: AtomicU32,
    backing_gain: AtomicU32,
    input_gain: AtomicU32,
    latency: AtomicU64,
    output_rate: AtomicU32,
    input_rate: AtomicU32,
    output_frames: AtomicU32,
    input_frames: AtomicU32,
    underruns: AtomicU64,
    input_drops: AtomicU64,
    stream_error: AtomicBool,
    loop_start: AtomicU64,
    loop_end: AtomicU64,
    loop_enabled: AtomicBool,
}
impl Shared {
    fn new() -> Self {
        Self {
            playback: ArrayQueue::new(PLAYBACK_CAPACITY),
            input: ArrayQueue::new(INPUT_CAPACITY),
            monitor: ArrayQueue::new(INPUT_CAPACITY),
            pitch: ArrayQueue::new(PITCH_CAPACITY),
            recording: RecordingBus::new(),
            replay: ArrayQueue::new(PLAYBACK_CAPACITY),
            replay_active: AtomicBool::new(false),
            replay_pending: AtomicBool::new(false),
            replay_generation: AtomicU64::new(1),
            replay_position: AtomicU64::new(0.0_f64.to_bits()),
            replay_duration: AtomicU64::new(0.0_f64.to_bits()),
            replay_error: AtomicBool::new(false),
            monitor_gain: AtomicU32::new(1.0_f32.to_bits()),
            voice_peak: AtomicU32::new(0),
            target_peak: AtomicU32::new(0),
            backing_peak: AtomicU32::new(0),
            origin: Instant::now(),
            callback_at: AtomicU64::new(0),
            output_lead: AtomicU64::new(0.0_f64.to_bits()),
            input_generation: AtomicU64::new(1),
            transport_epoch_at: AtomicU64::new(0),
            input_epoch_at: AtomicU64::new(0),
            output_active: AtomicBool::new(false),
            playing: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            input_active: AtomicBool::new(false),
            monitor_enabled: AtomicBool::new(false),
            generation: AtomicU64::new(1),
            monitor_generation: AtomicU64::new(1),
            position: AtomicU64::new(0.0_f64.to_bits()),
            iteration: AtomicU64::new(0),
            target_gain: AtomicU32::new(0.65_f32.to_bits()),
            backing_gain: AtomicU32::new(0.75_f32.to_bits()),
            input_gain: AtomicU32::new(1.0_f32.to_bits()),
            latency: AtomicU64::new(0.0_f64.to_bits()),
            output_rate: AtomicU32::new(48000),
            input_rate: AtomicU32::new(48000),
            output_frames: AtomicU32::new(0),
            input_frames: AtomicU32::new(0),
            underruns: AtomicU64::new(0),
            input_drops: AtomicU64::new(0),
            stream_error: AtomicBool::new(false),
            loop_start: AtomicU64::new(0.0_f64.to_bits()),
            loop_end: AtomicU64::new(0.0_f64.to_bits()),
            loop_enabled: AtomicBool::new(false),
        }
    }
    fn next_generation(&self) -> u64 {
        self.transport_epoch_at
            .store(self.origin.elapsed().as_nanos() as u64, Ordering::Release);
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }
    fn set_latency_compensation(&self, milliseconds: f32) {
        let seconds = if milliseconds.is_finite() {
            milliseconds as f64 / 1000.0
        } else {
            0.0
        };
        self.input_epoch_at
            .store(self.origin.elapsed().as_nanos() as u64, Ordering::Release);
        store_f64(&self.latency, seconds);
        self.input_generation.fetch_add(1, Ordering::AcqRel);
        while self.pitch.pop().is_some() {}
    }
    fn refresh_diagnostics(&self, diagnostics: &mut AudioDiagnostics) {
        diagnostics.output_buffer_frames = self.output_frames.load(Ordering::Relaxed);
        diagnostics.input_buffer_frames = self.input_frames.load(Ordering::Relaxed);
        diagnostics.underruns = self.underruns.load(Ordering::Relaxed);
        diagnostics.input_drops = self.input_drops.load(Ordering::Relaxed);
        diagnostics.stream_error = self.stream_error.load(Ordering::Relaxed);
        diagnostics.analysis_window_frames = 4096;
        diagnostics.analysis_delay_seconds = if self.input_active.load(Ordering::Acquire) {
            load_f64(&self.output_lead)
                + (3072.0 + diagnostics.input_buffer_frames as f64)
                    / self.input_rate.load(Ordering::Acquire).max(1) as f64
                + load_f64(&self.latency).max(0.0)
                + 0.005
        } else {
            0.0
        };
    }
    fn clear_playback(&self) {
        while self.playback.pop().is_some() {}
    }
}

enum Command {
    Tracks(Option<PathBuf>, Option<PathBuf>, u64, Sender<Result<()>>),
    Seek(f64, u64, Sender<Result<()>>),
    Shutdown,
}
enum ReplayCommand {
    Play(PathBuf, u64, Sender<Result<()>>),
    Shutdown,
}

/// Cloneable nonblocking access that never owns or initializes a CPAL stream.
#[derive(Clone)]
pub struct AudioHandle {
    shared: Arc<Shared>,
}
pub type CaptureEpoch = (u64, u64, u64);
impl AudioHandle {
    pub(crate) fn take_replay_error(&self) -> bool {
        self.shared.replay_error.swap(false, Ordering::AcqRel)
    }
    pub fn set_gains(&self, target: f32, backing: f32) {
        let finite = |gain: f32| {
            if gain.is_finite() {
                gain.clamp(0.0, 2.0)
            } else {
                0.0
            }
        };
        self.shared
            .target_gain
            .store(finite(target).to_bits(), Ordering::Relaxed);
        self.shared
            .backing_gain
            .store(finite(backing).to_bits(), Ordering::Relaxed);
    }
    pub fn mixer_levels(&self) -> [f32; 3] {
        [
            f32::from_bits(self.shared.voice_peak.swap(0, Ordering::Relaxed)),
            f32::from_bits(self.shared.target_peak.swap(0, Ordering::Relaxed)),
            f32::from_bits(self.shared.backing_peak.swap(0, Ordering::Relaxed)),
        ]
    }
    pub fn set_monitor_gain(&self, gain: f32) {
        self.shared.monitor_gain.store(
            if gain.is_finite() {
                gain.clamp(0.0, 2.0)
            } else {
                0.0
            }
            .to_bits(),
            Ordering::Relaxed,
        );
    }
    pub fn start_recording(&self, path: PathBuf) -> Result<RecordingSession> {
        if !self.is_input_running() {
            bail!("Connect the microphone before recording");
        }
        let session = self
            .shared
            .recording
            .start(path, self.shared.input_rate.load(Ordering::Acquire))?;
        if !self.is_input_running() {
            self.shared.recording.fail_input();
        }
        Ok(session)
    }
    pub fn recording_playback_state(&self) -> ReplaySnapshot {
        ReplaySnapshot {
            playing: self.shared.replay_active.load(Ordering::Acquire),
            pending: self.shared.replay_pending.load(Ordering::Acquire),
            position: load_f64(&self.shared.replay_position),
            duration: load_f64(&self.shared.replay_duration),
        }
    }
    pub fn stop_recording_playback(&self) {
        self.shared.replay_active.store(false, Ordering::Release);
        self.shared.replay_pending.store(false, Ordering::Release);
        self.shared.replay_generation.fetch_add(1, Ordering::AcqRel);
        while self.shared.replay.pop().is_some() {}
    }

    pub fn capture_epoch(&self) -> CaptureEpoch {
        (
            self.shared.generation.load(Ordering::Acquire),
            self.shared.iteration.load(Ordering::Acquire),
            self.shared.input_generation.load(Ordering::Acquire),
        )
    }
    pub fn drain_capture(&self) -> Vec<(PitchFrame, CaptureEpoch)> {
        let mut frames = Vec::new();
        while let Some(pitch) = self.shared.pitch.pop() {
            frames.push((
                pitch.frame,
                (pitch.generation, pitch.iteration, pitch.input_generation),
            ));
        }
        frames
    }
    /// Refreshes callback-owned fields using atomics only; no hardware API is queried.
    pub fn refresh_diagnostics(&self, diagnostics: &mut AudioDiagnostics) {
        self.shared.refresh_diagnostics(diagnostics);
    }
    pub fn position(&self) -> f64 {
        load_f64(&self.shared.position)
    }
    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Acquire)
    }
    pub fn is_input_running(&self) -> bool {
        self.shared.input_active.load(Ordering::Acquire)
    }
    pub fn loop_iteration(&self) -> u64 {
        self.shared.iteration.load(Ordering::Acquire)
    }
    pub fn pause(&self) -> f64 {
        let position = self.position();
        self.shared.playing.store(false, Ordering::Release);
        self.shared.next_generation();
        self.shared.iteration.store(0, Ordering::Release);
        position
    }
    pub fn set_latency_compensation(&self, milliseconds: f32) {
        self.shared.set_latency_compensation(milliseconds);
    }
    pub fn stop_input_capture(&self) {
        self.shared.recording.stop();
        self.shared.input_epoch_at.store(
            self.shared.origin.elapsed().as_nanos() as u64,
            Ordering::Release,
        );
        self.shared.input_active.store(false, Ordering::Release);
        self.shared.monitor_enabled.store(false, Ordering::Release);
        self.shared.input_generation.fetch_add(1, Ordering::AcqRel);
        self.shared.voice_peak.store(0, Ordering::Relaxed);
    }
    pub fn set_monitor(&self, enabled: bool) {
        self.shared
            .monitor_enabled
            .store(enabled && self.is_input_running(), Ordering::Release);
        self.shared
            .monitor_generation
            .fetch_add(1, Ordering::AcqRel);
        while self.shared.monitor.pop().is_some() {}
    }
    pub fn shutdown(&self) {
        self.shared.recording.stop();
        self.stop_recording_playback();
        self.pause();
        self.shared.monitor_enabled.store(false, Ordering::Release);
        self.shared.input_active.store(false, Ordering::Release);
        self.shared.shutdown.store(true, Ordering::Release);
    }
}

pub struct AudioEngine {
    shared: Arc<Shared>,
    commands: Sender<Command>,
    worker: Option<JoinHandle<()>>,
    capture_worker: Option<JoinHandle<()>>,
    replay_commands: Sender<ReplayCommand>,
    replay_worker: Option<JoinHandle<()>>,
    output: Option<cpal::Stream>,
    input: Option<cpal::Stream>,
    diagnostics: AudioDiagnostics,
    target_path: Option<PathBuf>,
    backing_path: Option<PathBuf>,
    preferences: AudioPreferences,
}
impl AudioEngine {
    pub fn new() -> Result<Self> {
        Self::new_with_preferences(&AudioPreferences::default())
    }
    pub fn new_with_preferences(preferences: &AudioPreferences) -> Result<Self> {
        let mut engine = Self::unconfigured()?;
        engine.configure_audio(preferences)?;
        Ok(engine)
    }
    /// Creates workers without calling a hardware API. Configure on a dedicated owner thread.
    pub fn unconfigured() -> Result<Self> {
        let shared = Arc::new(Shared::new());
        let (commands, receiver) = bounded(16);
        let worker_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("voice-playback-decoder".into())
            .spawn(move || decode_worker(receiver, worker_shared))?;
        let capture_shared = shared.clone();
        let capture_worker = thread::Builder::new()
            .name("voice-input-analysis".into())
            .spawn(move || input_worker(capture_shared))?;
        let (replay_commands, replay_receive) = bounded(4);
        let replay_shared = shared.clone();
        let replay_worker = thread::Builder::new()
            .name("voice-take-decoder".into())
            .spawn(move || replay_worker(replay_receive, replay_shared))?;
        let engine = Self {
            shared,
            commands,
            worker: Some(worker),
            capture_worker: Some(capture_worker),
            replay_commands,
            replay_worker: Some(replay_worker),
            output: None,
            input: None,
            diagnostics: AudioDiagnostics::default(),
            target_path: None,
            backing_path: None,
            preferences: AudioPreferences::default(),
        };
        Ok(engine)
    }
    pub fn handle(&self) -> AudioHandle {
        AudioHandle {
            shared: self.shared.clone(),
        }
    }
    pub fn play_recording(&self, path: PathBuf) -> Result<()> {
        if !self.shared.output_active.load(Ordering::Acquire) {
            bail!("The output stream is unavailable. Apply audio settings to reconnect.");
        }
        self.pause();
        self.handle().stop_recording_playback();
        let generation = self.shared.replay_generation.load(Ordering::Acquire);
        self.shared.replay_pending.store(true, Ordering::Release);
        let (reply, receive) = bounded(1);
        let result = self
            .replay_commands
            .send(ReplayCommand::Play(path, generation, reply))
            .map_err(anyhow::Error::from)
            .and_then(|_| {
                receive
                    .recv_timeout(Duration::from_secs(10))
                    .context("The take decoder did not respond within 10 seconds")?
            });
        if result.is_err() && self.shared.replay_generation.load(Ordering::Acquire) == generation {
            self.handle().stop_recording_playback();
        }
        result
    }
    pub fn stop_recording_playback(&self) {
        self.handle().stop_recording_playback();
    }
    pub fn set_monitor_gain(&self, gain: f32) {
        self.handle().set_monitor_gain(gain);
    }
    pub fn start_recording(&self, path: PathBuf) -> Result<RecordingSession> {
        self.handle().start_recording(path)
    }
    pub fn set_tracks(&mut self, target: Option<PathBuf>, backing: Option<PathBuf>) -> Result<()> {
        self.stop_recording_playback();
        self.pause();
        let generation = self.shared.next_generation();
        let (reply, receive) = bounded(1);
        self.commands.send(Command::Tracks(
            target.clone(),
            backing.clone(),
            generation,
            reply,
        ))?;
        match receive.recv_timeout(Duration::from_secs(10)) {
            Ok(result) => result?,
            Err(error) => {
                let _ = self.shared.generation.compare_exchange(
                    generation,
                    generation + 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                return Err(error).context("The decoder worker did not respond within 10 seconds");
            }
        }
        self.target_path = target;
        self.backing_path = backing;
        store_f64(&self.shared.position, 0.0);
        Ok(())
    }
    pub fn play(&self, offset: f64) -> Result<()> {
        if !self.shared.output_active.load(Ordering::Acquire) {
            bail!("The output stream is unavailable. Apply audio settings to reconnect.");
        }
        self.stop_recording_playback();
        self.seek(offset)?;
        self.shared.playing.store(true, Ordering::Release);
        Ok(())
    }
    pub fn pause(&self) -> f64 {
        let position = self.position();
        self.shared.playing.store(false, Ordering::Release);
        self.shared.next_generation();
        self.shared.iteration.store(0, Ordering::Release);
        store_f64(&self.shared.position, position);
        position
    }
    pub fn seek(&self, seconds: f64) -> Result<()> {
        if !seconds.is_finite() || seconds < 0.0 {
            bail!("Seek time must be finite and nonnegative");
        }
        let generation = self.shared.next_generation();
        let (reply, receive) = bounded(1);
        self.commands
            .send(Command::Seek(seconds, generation, reply))?;
        match receive.recv_timeout(Duration::from_secs(10)) {
            Ok(result) => result?,
            Err(error) => {
                let _ = self.shared.generation.compare_exchange(
                    generation,
                    generation + 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                return Err(error).context("The decoder worker did not respond within 10 seconds");
            }
        }
        self.shared.iteration.store(0, Ordering::Release);
        Ok(())
    }
    pub fn position(&self) -> f64 {
        load_f64(&self.shared.position)
    }
    pub fn loop_iteration(&self) -> u64 {
        self.shared.iteration.load(Ordering::Acquire)
    }
    pub fn set_latency_compensation(&self, milliseconds: f32) {
        self.shared.set_latency_compensation(milliseconds);
    }
    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Acquire)
    }
    pub fn set_loop(&self, start: f64, end: f64, enabled: bool) {
        let valid = start.is_finite() && end.is_finite() && start >= 0.0 && end - start >= 0.1;
        store_f64(&self.shared.loop_start, start.max(0.0));
        store_f64(&self.shared.loop_end, end.max(0.0));
        self.shared
            .loop_enabled
            .store(enabled && valid, Ordering::Release);
        let _ = self.seek(self.position());
    }
    pub fn set_gains(&self, target: f32, backing: f32) {
        self.handle().set_gains(target, backing);
    }
    pub fn configure_audio(&mut self, preferences: &AudioPreferences) -> Result<()> {
        self.stop_recording_playback();
        let playing = self.is_playing();
        let position = self.pause();
        self.stop_input();
        self.shared.output_active.store(false, Ordering::Release);
        self.output = None;
        let device = select_device(preferences.output_device.as_deref(), false)?;
        let supported = device.default_output_config()?;
        let mut config: StreamConfig = supported.config();
        config.buffer_size = requested_buffer(preferences.buffer_frames, supported.buffer_size());
        self.shared
            .output_rate
            .store(config.sample_rate, Ordering::Release);
        let stream = match build_output(
            &device,
            &config,
            supported.sample_format(),
            self.shared.clone(),
        ) {
            Ok(stream) => {
                self.diagnostics.buffer_fallback = false;
                stream
            }
            Err(_) if config.buffer_size != BufferSize::Default => {
                config.buffer_size = BufferSize::Default;
                self.diagnostics.buffer_fallback = true;
                build_output(
                    &device,
                    &config,
                    supported.sample_format(),
                    self.shared.clone(),
                )?
            }
            Err(error) => return Err(error),
        };
        stream.play()?;
        self.diagnostics.output_device = device_name(&device)?;
        self.diagnostics.sample_rate = config.sample_rate;
        self.diagnostics.output_channels = config.channels;
        self.diagnostics.requested_buffer_frames = preferences.buffer_frames;
        self.output = Some(stream);
        self.shared.output_active.store(true, Ordering::Release);
        self.preferences = preferences.clone();
        self.shared.stream_error.store(false, Ordering::Release);
        self.seek(position)?;
        if playing {
            self.shared.playing.store(true, Ordering::Release);
        }
        Ok(())
    }
    pub fn start_input(&mut self, preferences: &AudioPreferences) -> Result<()> {
        self.stop_input();
        let device = select_device(preferences.input_device.as_deref(), true)?;
        let supported = device.default_input_config()?;
        let mut config = supported.config();
        if preferences.input_channel >= config.channels {
            bail!(
                "The selected input exposes {} channels; channel {} is unavailable",
                config.channels,
                preferences.input_channel + 1
            );
        }
        config.buffer_size = requested_buffer(preferences.buffer_frames, supported.buffer_size());
        self.shared
            .input_rate
            .store(config.sample_rate, Ordering::Release);
        self.shared.input_gain.store(
            preferences.input_gain.clamp(0.0, 8.0).to_bits(),
            Ordering::Release,
        );
        self.set_latency_compensation(preferences.latency_ms);
        let stream = match build_input(
            &device,
            &config,
            supported.sample_format(),
            preferences.input_channel as usize,
            self.shared.clone(),
        ) {
            Ok(stream) => stream,
            Err(_) if config.buffer_size != BufferSize::Default => {
                config.buffer_size = BufferSize::Default;
                self.diagnostics.buffer_fallback = true;
                build_input(
                    &device,
                    &config,
                    supported.sample_format(),
                    preferences.input_channel as usize,
                    self.shared.clone(),
                )?
            }
            Err(error) => return Err(error),
        };
        stream.play()?;
        self.diagnostics.input_device = device_name(&device)?;
        self.diagnostics.input_sample_rate = config.sample_rate;
        self.diagnostics.input_channels = config.channels;
        self.preferences = preferences.clone();
        self.shared.input_active.store(true, Ordering::Release);
        self.input = Some(stream);
        self.set_monitor(false);
        Ok(())
    }
    pub fn is_input_running(&self) -> bool {
        self.shared.input_active.load(Ordering::Acquire) && self.input.is_some()
    }
    pub fn stop_input(&mut self) {
        self.shared.recording.stop();
        self.shared.voice_peak.store(0, Ordering::Relaxed);
        self.shared.input_epoch_at.store(
            self.shared.origin.elapsed().as_nanos() as u64,
            Ordering::Release,
        );
        self.shared.input_generation.fetch_add(1, Ordering::AcqRel);
        self.set_monitor(false);
        self.shared.input_active.store(false, Ordering::Release);
        self.input = None;
        while self.shared.input.pop().is_some() {}
        while self.shared.pitch.pop().is_some() {}
    }
    pub fn set_monitor(&self, enabled: bool) {
        self.shared.monitor_enabled.store(
            enabled && self.shared.input_active.load(Ordering::Acquire),
            Ordering::Release,
        );
        self.shared
            .monitor_generation
            .fetch_add(1, Ordering::AcqRel);
        while self.shared.monitor.pop().is_some() {}
    }
    pub fn drain_pitch(&self) -> Vec<PitchFrame> {
        let generation = self.shared.generation.load(Ordering::Acquire);
        let iteration = self.shared.iteration.load(Ordering::Acquire);
        let mut frames = Vec::new();
        while let Some(pitch) = self.shared.pitch.pop() {
            if pitch.generation == generation
                && pitch.iteration == iteration
                && pitch.input_generation == self.shared.input_generation.load(Ordering::Acquire)
            {
                frames.push(pitch.frame);
            }
        }
        frames
    }
    pub fn diagnostics(&self) -> AudioDiagnostics {
        let mut diagnostics = self.diagnostics.clone();
        self.shared.refresh_diagnostics(&mut diagnostics);
        diagnostics
    }
}
impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shared.recording.stop();
        self.shared.shutdown.store(true, Ordering::Release);
        self.shared.playing.store(false, Ordering::Release);
        self.input = None;
        self.output = None;
        let _ = self.commands.try_send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(worker) = self.capture_worker.take() {
            let _ = worker.join();
        }
        let _ = self.replay_commands.try_send(ReplayCommand::Shutdown);
        if let Some(worker) = self.replay_worker.take() {
            let _ = worker.join();
        }
    }
}

fn requested_buffer(frames: u32, supported: &SupportedBufferSize) -> BufferSize {
    if frames == 0 {
        return BufferSize::Default;
    }
    match supported {
        SupportedBufferSize::Range { min, max } => BufferSize::Fixed(frames.clamp(*min, *max)),
        SupportedBufferSize::Unknown => BufferSize::Fixed(frames),
    }
}
fn device_name(device: &cpal::Device) -> Result<String> {
    Ok(device.description()?.name().to_owned())
}

fn select_device(name: Option<&str>, input: bool) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = name {
        let mut devices = host.devices()?;
        return devices
            .find(|device| device_name(device).is_ok_and(|value| value == name))
            .with_context(|| format!("Audio device is unavailable: {name}"));
    }
    if input {
        host.default_input_device()
    } else {
        host.default_output_device()
    }
    .context("No default audio device is available")
}
/// Native property discovery avoids opening AudioUnits before stream configuration.
/// On macOS, CPAL 0.18 configuration queries read cached CoreAudio properties without creating a stream.
pub fn list_device_names() -> Result<DeviceInventory> {
    let host = cpal::default_host();
    let mut inventory = DeviceInventory::default();
    for device in host.devices()? {
        if let Ok(description) = device.description() {
            let info = DeviceInfo {
                name: description.name().to_owned(),
                ..Default::default()
            };
            if description.supports_input() {
                inventory
                    .inputs
                    .push(cached_device_configuration(info.clone(), &device, true));
            }
            if description.supports_output() {
                inventory
                    .outputs
                    .push(cached_device_configuration(info, &device, false));
            }
        }
    }
    Ok(inventory)
}

fn cached_device_configuration(
    info: DeviceInfo,
    _device: &cpal::Device,
    _input: bool,
) -> DeviceInfo {
    #[cfg(target_os = "macos")]
    {
        let mut info = info;
        let config = if _input {
            _device.default_input_config()
        } else {
            _device.default_output_config()
        };
        if let Ok(config) = config {
            fill_device_configuration(&mut info, &config);
        }
        info
    }
    #[cfg(not(target_os = "macos"))]
    {
        info
    }
}

#[cfg(any(target_os = "macos", test))]
fn fill_device_configuration(info: &mut DeviceInfo, config: &cpal::SupportedStreamConfig) {
    info.channels = config.channels();
    info.sample_rate_min = config.sample_rate();
    info.sample_rate_max = config.sample_rate();
    if let SupportedBufferSize::Range { min, max } = config.buffer_size() {
        info.buffer_min = Some(*min);
        info.buffer_max = Some(*max);
    }
}

pub fn list_devices() -> Result<DeviceInventory> {
    let host = cpal::default_host();
    let mut inventory = DeviceInventory::default();
    for device in host.devices()? {
        let name = device_name(&device).unwrap_or_else(|_| "Unnamed audio device".into());
        for input in [true, false] {
            let ranges: Vec<_> = if input {
                device
                    .supported_input_configs()
                    .map(|ranges| ranges.collect())
                    .unwrap_or_default()
            } else {
                device
                    .supported_output_configs()
                    .map(|ranges| ranges.collect())
                    .unwrap_or_default()
            };
            if ranges.is_empty() {
                continue;
            }
            let mut info = DeviceInfo {
                name: name.clone(),
                channels: 0,
                sample_rate_min: u32::MAX,
                sample_rate_max: 0,
                buffer_min: None,
                buffer_max: None,
            };
            for range in ranges {
                info.channels = info.channels.max(range.channels());
                info.sample_rate_min = info.sample_rate_min.min(range.min_sample_rate());
                info.sample_rate_max = info.sample_rate_max.max(range.max_sample_rate());
                if let SupportedBufferSize::Range { min, max } = range.buffer_size() {
                    info.buffer_min = Some(info.buffer_min.map_or(*min, |old| old.min(*min)));
                    info.buffer_max = Some(info.buffer_max.map_or(*max, |old| old.max(*max)));
                }
            }
            if input {
                inventory.inputs.push(info);
            } else {
                inventory.outputs.push(info);
            }
        }
    }
    Ok(inventory)
}

struct ResampledReader {
    reader: AudioReader,
    block: Vec<[f32; 2]>,
    index: usize,
    a: Option<[f32; 2]>,
    b: Option<[f32; 2]>,
    phase: f64,
    rate: u32,
    started: bool,
}
impl ResampledReader {
    fn open(path: PathBuf) -> Result<Self> {
        let reader = AudioReader::open(&path)?;
        let rate = reader.metadata().sample_rate;
        Ok(Self {
            reader,
            block: Vec::new(),
            index: 0,
            a: None,
            b: None,
            phase: 0.0,
            rate,
            started: false,
        })
    }
    fn seek(&mut self, time: f64) -> Result<()> {
        self.reader.seek(time)?;
        self.block.clear();
        self.index = 0;
        self.a = None;
        self.b = None;
        self.phase = 0.0;
        self.started = false;
        Ok(())
    }
    fn finished(&self) -> bool {
        self.started && self.index >= self.block.len() && self.a.is_none() && self.b.is_none()
    }
    fn native(&mut self) -> Result<Option<[f32; 2]>> {
        if self.index >= self.block.len() {
            self.block = self.reader.read_stereo(4096)?;
            self.index = 0;
        }
        let value = self.block.get(self.index).copied();
        self.index += usize::from(value.is_some());
        Ok(value)
    }
    fn next(&mut self, output_rate: u32) -> Result<[f32; 2]> {
        self.started = true;
        if self.a.is_none() {
            self.a = self.native()?;
            self.b = self.native()?;
        }
        let Some(a) = self.a else {
            return Ok([0.0; 2]);
        };
        let b = self.b.unwrap_or(a);
        let value = [
            a[0] + (b[0] - a[0]) * self.phase as f32,
            a[1] + (b[1] - a[1]) * self.phase as f32,
        ];
        self.phase += self.rate as f64 / output_rate as f64;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
            self.a = self.b;
            self.b = self.native()?;
        }
        Ok(value)
    }
}
fn seek_readers(
    target: &mut Option<ResampledReader>,
    backing: &mut Option<ResampledReader>,
    time: f64,
) -> Result<()> {
    if let Some(reader) = target {
        reader.seek(time)?;
    }
    if let Some(reader) = backing {
        reader.seek(time)?;
    }
    Ok(())
}
fn decode_worker(commands: Receiver<Command>, shared: Arc<Shared>) {
    let mut target: Option<ResampledReader> = None;
    let mut backing: Option<ResampledReader> = None;
    let mut duration = 0.0;
    let mut cursor = 0.0;
    let mut generation = 1;
    let mut iteration = 0;
    while !shared.shutdown.load(Ordering::Acquire) {
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Shutdown => return,
                Command::Tracks(t, b, g, reply) => {
                    let result = (|| -> Result<_> {
                        let t = t.map(ResampledReader::open).transpose()?;
                        let b = b.map(ResampledReader::open).transpose()?;
                        let duration = if t
                            .as_ref()
                            .is_some_and(|r| r.reader.metadata().duration == 0.0)
                            || b.as_ref()
                                .is_some_and(|r| r.reader.metadata().duration == 0.0)
                        {
                            0.0
                        } else {
                            t.as_ref()
                                .map_or(0.0, |r| r.reader.metadata().duration)
                                .max(b.as_ref().map_or(0.0, |r| r.reader.metadata().duration))
                        };
                        Ok((t, b, duration))
                    })();
                    match result {
                        Ok((t, b, d)) if g == shared.generation.load(Ordering::Acquire) => {
                            target = t;
                            backing = b;
                            duration = d;
                            cursor = 0.0;
                            generation = g;
                            iteration = 0;
                            shared.clear_playback();
                            let _ = reply.send(Ok(()));
                        }
                        Ok(_) => {
                            let _ = reply
                                .send(Err(anyhow::anyhow!("The track request was superseded")));
                        }
                        Err(error) => {
                            let _ = reply.send(Err(error));
                        }
                    }
                }
                Command::Seek(time, g, reply) => {
                    shared.clear_playback();
                    generation = g;
                    iteration = 0;
                    cursor = if duration > 0.0 {
                        time.min(duration)
                    } else {
                        time
                    };
                    let mut result = seek_readers(&mut target, &mut backing, cursor);
                    if g == shared.generation.load(Ordering::Acquire) {
                        store_f64(&shared.position, cursor);
                    } else {
                        result = Err(anyhow::anyhow!("The seek request was superseded"));
                    }
                    let _ = reply.send(result);
                }
            }
        }
        if !shared.playing.load(Ordering::Acquire)
            || shared.playback.len() >= PLAYBACK_CAPACITY - 1024
        {
            thread::sleep(Duration::from_millis(2));
            continue;
        }
        let rate = shared.output_rate.load(Ordering::Acquire).max(1);
        for _ in 0..512 {
            if shared.playback.is_full() || generation != shared.generation.load(Ordering::Acquire)
            {
                break;
            }
            if shared.loop_enabled.load(Ordering::Acquire) {
                let end = load_f64(&shared.loop_end);
                if cursor >= end {
                    cursor = load_f64(&shared.loop_start);
                    iteration += 1;
                    if seek_readers(&mut target, &mut backing, cursor).is_err() {
                        shared.stream_error.store(true, Ordering::Release);
                        shared.playing.store(false, Ordering::Release);
                        break;
                    }
                }
            }
            let ended = if duration > 0.0 {
                cursor >= duration
            } else {
                target.as_ref().is_none_or(ResampledReader::finished)
                    && backing.as_ref().is_none_or(ResampledReader::finished)
            };
            let result = (|| -> Result<MixFrame> {
                Ok(MixFrame {
                    generation,
                    position: cursor,
                    target: if ended {
                        [0.0; 2]
                    } else if let Some(reader) = &mut target {
                        reader.next(rate)?
                    } else {
                        [0.0; 2]
                    },
                    backing: if ended {
                        [0.0; 2]
                    } else if let Some(reader) = &mut backing {
                        reader.next(rate)?
                    } else {
                        [0.0; 2]
                    },
                    ended,
                    iteration,
                })
            })();
            match result {
                Ok(frame) => {
                    let _ = shared.playback.push(frame);
                    if ended {
                        break;
                    }
                    cursor += 1.0 / rate as f64;
                }
                Err(_) => {
                    shared.stream_error.store(true, Ordering::Release);
                    shared.playing.store(false, Ordering::Release);
                    break;
                }
            }
        }
    }
}

struct ReplayReader {
    reader: RecordingReader,
    block: Vec<f32>,
    index: usize,
    a: Option<f32>,
    b: Option<f32>,
    phase: f64,
}
impl ReplayReader {
    fn native(&mut self) -> Result<Option<f32>> {
        if self.index >= self.block.len() {
            self.block = self.reader.read_mono(4096)?;
            self.index = 0;
        }
        let sample = self.block.get(self.index).copied();
        self.index += usize::from(sample.is_some());
        Ok(sample)
    }
    fn next(&mut self, output_rate: u32) -> Result<f32> {
        if self.a.is_none() {
            self.a = self.native()?;
            self.b = self.native()?;
        }
        let Some(a) = self.a else {
            return Ok(0.0);
        };
        let value = a + (self.b.unwrap_or(a) - a) * self.phase as f32;
        self.phase += self.reader.sample_rate as f64 / output_rate as f64;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
            self.a = self.b;
            self.b = self.native()?;
        }
        Ok(value)
    }
}
fn replay_worker(receive: Receiver<ReplayCommand>, shared: Arc<Shared>) {
    let mut reader: Option<ReplayReader> = None;
    let mut cursor = 0.0;
    let mut duration = 0.0;
    let mut generation = 0;
    let mut eof_queued = false;
    while !shared.shutdown.load(Ordering::Acquire) {
        while let Ok(command) = receive.try_recv() {
            match command {
                ReplayCommand::Shutdown => return,
                ReplayCommand::Play(path, requested, reply) => {
                    let result = RecordingReader::open(&path);
                    if requested != shared.replay_generation.load(Ordering::Acquire) {
                        let _ = reply.send(Err(anyhow::anyhow!(
                            "The take playback request was superseded"
                        )));
                        continue;
                    }
                    match result {
                        Ok(recording) => {
                            duration = recording.frames as f64 / recording.sample_rate as f64;
                            reader = Some(ReplayReader {
                                reader: recording,
                                block: Vec::new(),
                                index: 0,
                                a: None,
                                b: None,
                                phase: 0.0,
                            });
                            cursor = 0.0;
                            generation = requested;
                            eof_queued = false;
                            while shared.replay.pop().is_some() {}
                            store_f64(&shared.replay_position, 0.0);
                            store_f64(&shared.replay_duration, duration);
                            shared.replay_pending.store(false, Ordering::Release);
                            shared.replay_active.store(true, Ordering::Release);
                            let _ = reply.send(Ok(()));
                        }
                        Err(error) => {
                            shared.replay_pending.store(false, Ordering::Release);
                            let _ = reply.send(Err(error));
                        }
                    }
                }
            }
        }
        if generation != shared.replay_generation.load(Ordering::Acquire) {
            reader = None;
        }
        if !shared.replay_active.load(Ordering::Acquire)
            || eof_queued
            || reader.is_none()
            || shared.replay.len() >= PLAYBACK_CAPACITY - 1024
        {
            thread::sleep(Duration::from_millis(2));
            continue;
        }
        let rate = shared.output_rate.load(Ordering::Acquire).max(1);
        for _ in 0..512 {
            if generation != shared.replay_generation.load(Ordering::Acquire)
                || shared.replay.is_full()
            {
                break;
            }
            let ended = cursor >= duration;
            let result = if ended {
                Ok(0.0)
            } else {
                reader.as_mut().expect("active replay reader").next(rate)
            };
            match result {
                Ok(value) => {
                    let _ = shared.replay.push(ReplayFrame {
                        generation,
                        position: cursor.min(duration),
                        value,
                        ended,
                    });
                    if ended {
                        eof_queued = true;
                        break;
                    }
                    cursor += 1.0 / rate as f64;
                }
                Err(_) => {
                    shared.replay_error.store(true, Ordering::Release);
                    shared.replay_active.store(false, Ordering::Release);
                    reader = None;
                    break;
                }
            }
        }
    }
}
fn render_replay(shared: &Shared) -> f32 {
    if !shared.replay_active.load(Ordering::Acquire) {
        return 0.0;
    }
    if let Some(frame) = shared.replay.pop() {
        if frame.generation != shared.replay_generation.load(Ordering::Acquire) {
            return 0.0;
        }
        store_f64(&shared.replay_position, frame.position);
        if frame.ended {
            shared.replay_active.store(false, Ordering::Release);
            return 0.0;
        }
        store_f64(
            &shared.replay_position,
            (frame.position + 1.0 / shared.output_rate.load(Ordering::Relaxed).max(1) as f64)
                .min(load_f64(&shared.replay_duration)),
        );
        peak(
            &shared.voice_peak,
            frame.value * load_f32(&shared.monitor_gain),
        );
        return frame.value;
    }
    shared.underruns.fetch_add(1, Ordering::Relaxed);
    0.0
}
fn peak(atomic: &AtomicU32, value: f32) {
    if value.is_finite() {
        atomic.fetch_max(value.abs().to_bits(), Ordering::Relaxed);
    }
}

fn render_frame(shared: &Shared) -> [f32; 2] {
    if !shared.playing.load(Ordering::Acquire) {
        return [0.0; 2];
    }
    let generation = shared.generation.load(Ordering::Acquire);
    if let Some(frame) = shared.playback.pop() {
        if frame.generation != generation {
            return [0.0; 2];
        }
        if frame.ended {
            shared.playing.store(false, Ordering::Release);
            store_f64(&shared.position, frame.position);
            return [0.0; 2];
        }
        store_f64(
            &shared.position,
            frame.position + 1.0 / shared.output_rate.load(Ordering::Relaxed).max(1) as f64,
        );
        shared.iteration.store(frame.iteration, Ordering::Release);
        let t = load_f32(&shared.target_gain);
        let b = load_f32(&shared.backing_gain);
        peak(
            &shared.target_peak,
            frame.target[0].abs().max(frame.target[1].abs()) * t,
        );
        peak(
            &shared.backing_peak,
            frame.backing[0].abs().max(frame.backing[1].abs()) * b,
        );
        return [
            frame.target[0] * t + frame.backing[0] * b,
            frame.target[1] * t + frame.backing[1] * b,
        ];
    }
    shared.underruns.fetch_add(1, Ordering::Relaxed);
    [0.0; 2]
}
struct MonitorState {
    a: f32,
    b: f32,
    phase: f64,
    generation: u64,
    ready: bool,
}
impl MonitorState {
    fn new() -> Self {
        Self {
            a: 0.0,
            b: 0.0,
            phase: 0.0,
            generation: 0,
            ready: false,
        }
    }
    fn next(&mut self, shared: &Shared) -> f32 {
        let generation = shared.monitor_generation.load(Ordering::Relaxed);
        if generation != self.generation {
            self.ready = false;
            self.phase = 0.0;
            self.generation = generation;
        }
        if !shared.monitor_enabled.load(Ordering::Relaxed) {
            return 0.0;
        }
        if !self.ready {
            self.a = shared.monitor.pop().unwrap_or(0.0);
            self.b = shared.monitor.pop().unwrap_or(0.0);
            self.ready = true;
        }
        let value = self.a + (self.b - self.a) * self.phase as f32;
        self.phase += shared.input_rate.load(Ordering::Relaxed).max(1) as f64
            / shared.output_rate.load(Ordering::Relaxed).max(1) as f64;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
            self.a = self.b;
            self.b = shared.monitor.pop().unwrap_or(0.0);
        }
        value
    }
}
fn render_output<T>(data: &mut [T], channels: usize, shared: &Shared, monitor: &mut MonitorState)
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    shared
        .output_frames
        .store((data.len() / channels) as u32, Ordering::Relaxed);
    for output in data.chunks_exact_mut(channels) {
        let mix = render_frame(shared);
        let live = (monitor.next(shared) + render_replay(shared)) * load_f32(&shared.monitor_gain);
        for (channel, sample) in output.iter_mut().enumerate() {
            let value = if channels == 1 {
                (mix[0] + mix[1]) * 0.5
            } else if channel < 2 {
                mix[channel]
            } else {
                0.0
            };
            *sample = T::from_sample(safe_output(value + live));
        }
    }
}
fn output_error(shared: &Shared) {
    shared.stream_error.store(true, Ordering::Release);
    shared.playing.store(false, Ordering::Release);
    shared.output_active.store(false, Ordering::Release);
    shared.replay_active.store(false, Ordering::Release);
}
#[cfg(test)]
pub(crate) fn output_callback_for_test(handle: &AudioHandle, frames: usize, running: bool) {
    let mut data = vec![0.0_f32; frames * 2];
    let mut monitor = MonitorState::new();
    handle.shared.playing.store(running, Ordering::Release);
    render_output(&mut data, 2, &handle.shared, &mut monitor);
}
#[cfg(test)]
pub(crate) fn output_error_for_test(handle: &AudioHandle) {
    output_error(&handle.shared);
}

fn output_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    shared: Arc<Shared>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let errors = shared.clone();
    let mut monitor = MonitorState::new();
    Ok(device.build_output_stream(
        *config,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            let frames = data.len() / channels;
            let latency = info
                .timestamp()
                .playback
                .checked_duration_since(info.timestamp().callback)
                .map_or(0.0, |d| d.as_secs_f64());
            store_f64(
                &shared.output_lead,
                latency + frames as f64 / shared.output_rate.load(Ordering::Relaxed).max(1) as f64,
            );
            shared
                .callback_at
                .store(shared.origin.elapsed().as_nanos() as u64, Ordering::Release);
            render_output(data, channels, &shared, &mut monitor);
        },
        move |_| output_error(&errors),
        Some(Duration::from_secs(3)),
    )?)
}
fn build_output(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    shared: Arc<Shared>,
) -> Result<cpal::Stream> {
    match format {
        SampleFormat::F32 => output_stream::<f32>(device, config, shared),
        SampleFormat::F64 => output_stream::<f64>(device, config, shared),
        SampleFormat::I16 => output_stream::<i16>(device, config, shared),
        SampleFormat::I32 => output_stream::<i32>(device, config, shared),
        SampleFormat::U16 => output_stream::<u16>(device, config, shared),
        _ => bail!("Unsupported output sample format: {format:?}"),
    }
}
fn capture_input<T>(
    data: &[T],
    channels: usize,
    channel: usize,
    rate: u32,
    shared: &Shared,
    callback_clock: f64,
    capture_age: f64,
) where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    shared
        .input_frames
        .store((data.len() / channels) as u32, Ordering::Relaxed);
    if !shared.input_active.load(Ordering::Acquire) {
        return;
    }
    let elapsed =
        callback_clock - shared.callback_at.load(Ordering::Acquire) as f64 / 1_000_000_000.0;
    let delta = elapsed - load_f64(&shared.output_lead) - capture_age - load_f64(&shared.latency);
    let position = load_f64(&shared.position);
    let generation = shared.generation.load(Ordering::Acquire);
    let iteration = shared.iteration.load(Ordering::Acquire);
    let input_generation = shared.input_generation.load(Ordering::Acquire);
    let gain = load_f32(&shared.input_gain);
    let playing = shared.playing.load(Ordering::Acquire);
    let loop_region = if shared.loop_enabled.load(Ordering::Acquire) {
        Some((load_f64(&shared.loop_start), load_f64(&shared.loop_end)))
    } else {
        None
    };
    let epoch_at = shared
        .transport_epoch_at
        .load(Ordering::Acquire)
        .max(shared.input_epoch_at.load(Ordering::Acquire)) as f64
        / 1_000_000_000.0;
    let recording = shared.recording.enter();
    for (index, frame) in data.chunks_exact(channels).enumerate() {
        let raw = cpal::Sample::to_sample::<f32>(frame[channel]) * gain;
        let value = if raw.is_finite() { raw } else { 0.0 };
        peak(&shared.voice_peak, value);
        let capture_clock = callback_clock - capture_age + index as f64 / rate as f64;
        if capture_clock < epoch_at {
            continue;
        }
        // An explicitly armed practice recording begins with transport, excluding pre-roll.
        if playing && let Some(tap) = &recording {
            tap.push(value);
        }
        let (time, sample_iteration) = if playing {
            map_capture_time(
                position,
                iteration,
                delta + index as f64 / rate as f64,
                loop_region,
            )
        } else {
            (position, iteration)
        };
        if shared
            .input
            .push(InputFrame {
                value,
                time,
                generation,
                iteration: sample_iteration,
                input_generation,
                playing,
            })
            .is_err()
        {
            shared.input_drops.fetch_add(1, Ordering::Relaxed);
        }
        if shared.monitor_enabled.load(Ordering::Relaxed) {
            let _ = shared.monitor.push(value);
        }
    }
}

fn input_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    channel: usize,
    shared: Arc<Shared>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels as usize;
    let rate = config.sample_rate;
    let errors = shared.clone();
    Ok(device.build_input_stream(
        *config,
        move |data: &[T], info: &cpal::InputCallbackInfo| {
            let capture_age = info
                .timestamp()
                .callback
                .checked_duration_since(info.timestamp().capture)
                .map_or(0.0, |duration| duration.as_secs_f64());
            capture_input(
                data,
                channels,
                channel,
                rate,
                &shared,
                shared.origin.elapsed().as_secs_f64(),
                capture_age,
            );
        },
        move |_| {
            errors.stream_error.store(true, Ordering::Release);
            errors.recording.fail_input();
            errors.input_active.store(false, Ordering::Release);
            errors.input_generation.fetch_add(1, Ordering::AcqRel);
            errors.monitor_enabled.store(false, Ordering::Release);
        },
        Some(Duration::from_secs(3)),
    )?)
}
fn build_input(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    channel: usize,
    shared: Arc<Shared>,
) -> Result<cpal::Stream> {
    match format {
        SampleFormat::F32 => input_stream::<f32>(device, config, channel, shared),
        SampleFormat::F64 => input_stream::<f64>(device, config, channel, shared),
        SampleFormat::I16 => input_stream::<i16>(device, config, channel, shared),
        SampleFormat::I32 => input_stream::<i32>(device, config, channel, shared),
        SampleFormat::U16 => input_stream::<u16>(device, config, channel, shared),
        _ => bail!("Unsupported input sample format: {format:?}"),
    }
}
fn input_worker(shared: Arc<Shared>) {
    let mut window = [0.0_f32; 4096];
    let mut index = 0;
    let mut count = 0;
    let mut hop = 0;
    let mut generation = 0;
    let mut iteration = 0;
    let mut input_generation = 0;
    while !shared.shutdown.load(Ordering::Acquire) {
        let Some(sample) = shared.input.pop() else {
            thread::sleep(Duration::from_millis(2));
            continue;
        };
        if sample.generation != shared.generation.load(Ordering::Acquire)
            || sample.input_generation != shared.input_generation.load(Ordering::Acquire)
        {
            continue;
        }
        if sample.generation != generation
            || sample.iteration != iteration
            || sample.input_generation != input_generation
        {
            index = 0;
            count = 0;
            hop = 0;
            generation = sample.generation;
            iteration = sample.iteration;
            input_generation = sample.input_generation;
        }
        window[index] = sample.value;
        index = (index + 1) % window.len();
        count += 1;
        hop += 1;
        if count >= window.len() && hop >= 1024 {
            hop = 0;
            let mut ordered = Vec::with_capacity(window.len());
            ordered.extend_from_slice(&window[index..]);
            ordered.extend_from_slice(&window[..index]);
            let rate = shared.input_rate.load(Ordering::Acquire);
            let frame = detect_pitch(
                &ordered,
                rate,
                if sample.playing {
                    sample.time - (window.len() as f64 / 2.0) / rate as f64
                } else {
                    sample.time
                },
            );
            let _ = shared.pitch.push(TaggedPitch {
                frame,
                generation,
                iteration,
                input_generation,
            });
        }
    }
}

/// Convert a capture-clock offset before loop wrapping, preserving the corresponding attempt.
fn map_capture_time(
    position: f64,
    iteration: u64,
    delta: f64,
    region: Option<(f64, f64)>,
) -> (f64, u64) {
    let Some((start, end)) = region.filter(|(start, end)| end > start) else {
        return ((position + delta).max(0.0), 0);
    };
    let length = end - start;
    let raw = if iteration == 0 {
        position
    } else {
        end + (iteration - 1) as f64 * length + position - start
    } + delta;
    if raw < end {
        (raw.max(0.0), 0)
    } else {
        (
            start + (raw - end).rem_euclid(length),
            1 + ((raw - end) / length).floor() as u64,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn finish_recording(session: &RecordingSession) -> crate::recording::RecordedAudioSummary {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(result) = session.try_finish() {
                return result.unwrap();
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn capture_records_selected_channel_on_transport_independently_of_monitor_fader() {
        let dir = tempfile::tempdir().unwrap();
        let shared = Arc::new(Shared::new());
        let handle = AudioHandle {
            shared: shared.clone(),
        };
        assert!(
            handle
                .start_recording(dir.path().join("without-mic.wav"))
                .is_err()
        );
        shared.input_active.store(true, Ordering::Release);
        shared.input_rate.store(8_000, Ordering::Release);
        shared
            .input_gain
            .store(2.0_f32.to_bits(), Ordering::Release);
        handle.set_monitor_gain(0.0);
        let session = handle
            .start_recording(dir.path().join("selected.wav"))
            .unwrap();
        // Metering continues while idle, but practice recording excludes pre-roll.
        capture_input(&[0.9_f32, 0.25], 2, 1, 8_000, &shared, 1.0, 0.0);
        shared.playing.store(true, Ordering::Release);
        capture_input(
            &[0.9_f32, 0.125, 0.8, -0.25],
            2,
            1,
            8_000,
            &shared,
            1.0,
            0.0,
        );
        shared.playing.store(false, Ordering::Release);
        capture_input(&[0.9_f32, 0.75], 2, 1, 8_000, &shared, 1.0, 0.0);
        session.stop();
        let summary = finish_recording(&session);
        assert_eq!(summary.frames, 2);
        assert_eq!(summary.sample_rate, 8_000);
        let mut reader = RecordingReader::open(&summary.path).unwrap();
        assert_eq!(reader.read_mono(100).unwrap(), vec![0.25, -0.5]);
        assert!(shared.monitor.is_empty());
        assert_eq!(handle.mixer_levels(), [1.5, 0.0, 0.0]);
        assert_eq!(handle.mixer_levels(), [0.0; 3]);
        handle.stop_input_capture();
        assert!(!handle.is_input_running());
    }
    #[test]
    fn replay_and_monitor_share_only_voice_fader_without_changing_track_gains_or_cursor() {
        let shared = Arc::new(Shared::new());
        let handle = AudioHandle {
            shared: shared.clone(),
        };
        store_f64(&shared.position, 12.0);
        shared.replay_active.store(true, Ordering::Release);
        store_f64(&shared.replay_duration, 1.0);
        handle.set_monitor_gain(0.25);
        shared
            .replay
            .push(ReplayFrame {
                generation: 1,
                position: 0.0,
                value: 0.8,
                ended: false,
            })
            .ok();
        let mut output = [0.0_f32; 2];
        render_output(&mut output, 2, &shared, &mut MonitorState::new());
        assert_eq!(output, [0.2, 0.2]);
        assert_eq!(handle.position(), 12.0);
        assert_eq!(load_f32(&shared.input_gain), 1.0);
        assert_eq!(handle.mixer_levels(), [0.2, 0.0, 0.0]);
        handle.stop_recording_playback();
        // A stale decoder frame cannot play or move either transport after cancellation.
        shared
            .replay
            .push(ReplayFrame {
                generation: 1,
                position: 20.0,
                value: 1.0,
                ended: false,
            })
            .ok();
        render_output(&mut output, 2, &shared, &mut MonitorState::new());
        assert_eq!(output, [0.0; 2]);
        assert_eq!(handle.position(), 12.0);
        assert!(!handle.recording_playback_state().playing);
    }
    #[test]
    fn replay_streams_recorded_file_resamples_and_finishes_without_touching_main_transport() {
        let dir = tempfile::tempdir().unwrap();
        let engine = AudioEngine::unconfigured().unwrap();
        let shared = &engine.shared;
        shared.input_active.store(true, Ordering::Release);
        shared.input_rate.store(8_000, Ordering::Release);
        shared.output_rate.store(16_000, Ordering::Release);
        shared.output_active.store(true, Ordering::Release);
        shared.playing.store(true, Ordering::Release);
        let session = engine
            .start_recording(dir.path().join("replay.wav"))
            .unwrap();
        let samples = [0.0_f32, 0.25, 0.5, 0.75];
        capture_input(&samples, 1, 0, 8_000, shared, 1.0, 0.0);
        session.stop();
        let summary = finish_recording(&session);
        engine.pause();
        store_f64(&shared.position, 12.0);
        engine.play_recording(summary.path).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while shared.replay.len() < 9 {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
        let mut output = [0.0_f32; 18];
        render_output(&mut output, 2, shared, &mut MonitorState::new());
        let mono: Vec<_> = output.as_chunks::<2>().0.iter().map(|f| f[0]).collect();
        assert_eq!(
            mono,
            vec![0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.75, 0.0]
        );
        assert!(!engine.handle().recording_playback_state().playing);
        assert!(!engine.is_playing());
        assert_eq!(engine.position(), 12.0);
        assert!(
            engine
                .play_recording(dir.path().join("missing.wav"))
                .is_err()
        );
        assert!(!engine.handle().recording_playback_state().pending);
    }
    #[test]
    fn mixer_uses_one_clock_and_rejects_late_generations() {
        let shared = Shared::new();
        shared.playing.store(true, Ordering::Release);
        shared.output_rate.store(100, Ordering::Release);
        shared
            .target_gain
            .store(0.5_f32.to_bits(), Ordering::Relaxed);
        shared
            .backing_gain
            .store(0.25_f32.to_bits(), Ordering::Relaxed);
        shared
            .playback
            .push(MixFrame {
                generation: 1,
                position: 2.0,
                target: [0.4, -0.4],
                backing: [0.8, 0.4],
                ..Default::default()
            })
            .ok();
        assert_eq!(render_frame(&shared), [0.4, -0.1]);
        assert!((load_f64(&shared.position) - 2.01).abs() < 1e-9);
        shared.generation.store(2, Ordering::Release);
        shared
            .playback
            .push(MixFrame {
                generation: 1,
                position: 99.0,
                target: [1.0; 2],
                ..Default::default()
            })
            .ok();
        assert_eq!(render_frame(&shared), [0.0; 2]);
        assert!((load_f64(&shared.position) - 2.01).abs() < 1e-9);
        assert_eq!(render_frame(&shared), [0.0; 2]);
        assert_eq!(shared.underruns.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn playback_queue_is_bounded_and_monitor_starts_silent() {
        let shared = Shared::new();
        for _ in 0..PLAYBACK_CAPACITY {
            shared.playback.push(MixFrame::default()).ok();
        }
        assert!(shared.playback.push(MixFrame::default()).is_err());
        let mut monitor = MonitorState::new();
        shared.monitor.push(1.0).ok();
        assert_eq!(monitor.next(&shared), 0.0);
        shared.clear_playback();
        assert!(shared.playback.is_empty());
    }
    #[test]
    fn cached_duplex_configuration_keeps_input_and_output_channels_separate() {
        let device = DeviceInfo {
            name: "Duplex interface".into(),
            ..Default::default()
        };
        let mut input = device.clone();
        let mut output = device;
        fill_device_configuration(
            &mut input,
            &cpal::SupportedStreamConfig::new(
                2,
                44_100,
                SupportedBufferSize::Range { min: 32, max: 512 },
                SampleFormat::F32,
            ),
        );
        fill_device_configuration(
            &mut output,
            &cpal::SupportedStreamConfig::new(
                6,
                96_000,
                SupportedBufferSize::Range { min: 64, max: 1024 },
                SampleFormat::F32,
            ),
        );
        assert_eq!(input.name, output.name);
        assert_eq!(
            (input.channels, input.sample_rate_min, input.sample_rate_max),
            (2, 44_100, 44_100)
        );
        assert_eq!(
            (
                output.channels,
                output.sample_rate_min,
                output.sample_rate_max
            ),
            (6, 96_000, 96_000)
        );
        assert_eq!((input.buffer_min, input.buffer_max), (Some(32), Some(512)));
        assert_eq!(
            (output.buffer_min, output.buffer_max),
            (Some(64), Some(1024))
        );
    }
    #[test]
    fn buffer_request_respects_known_hardware_range() {
        assert_eq!(
            requested_buffer(32, &SupportedBufferSize::Range { min: 64, max: 1024 }),
            BufferSize::Fixed(64)
        );
        assert_eq!(
            requested_buffer(2048, &SupportedBufferSize::Range { min: 64, max: 1024 }),
            BufferSize::Fixed(1024)
        );
        assert_eq!(
            requested_buffer(0, &SupportedBufferSize::Unknown),
            BufferSize::Default
        );
    }
    #[test]
    fn capture_compensation_wraps_before_assigning_the_loop_attempt() {
        let (time, iteration) = map_capture_time(4.02, 1, -0.1, Some((4.0, 8.0)));
        assert!((time - 7.92).abs() < 1e-9);
        assert_eq!(iteration, 0);
        let (time, iteration) = map_capture_time(4.02, 1, 0.1, Some((4.0, 8.0)));
        assert!((time - 4.12).abs() < 1e-9);
        assert_eq!(iteration, 1);
    }
    #[test]
    fn streaming_resampler_preserves_seek_origin_and_rate_ratio() {
        let samples: Vec<_> = (0..800).map(|i| [i as i16, -(i as i16)]).collect();
        let file = crate::media::tests::wav(&samples, 8000);
        let mut reader = ResampledReader::open(file.path().to_owned()).unwrap();
        reader.seek(0.025).unwrap();
        let first = reader.next(16000).unwrap();
        let second = reader.next(16000).unwrap();
        assert!((first[0] - 200.0 / 32768.0).abs() < 1e-7);
        assert!((second[0] - 200.5 / 32768.0).abs() < 1e-7);
        reader.seek(0.1).unwrap();
        assert_eq!(reader.next(16000).unwrap(), [0.0; 2]);
    }
    #[test]
    fn decoder_worker_reports_file_failure_and_shuts_down_without_hardware() {
        let shared = Arc::new(Shared::new());
        let (send, receive) = bounded(2);
        let state = shared.clone();
        let worker = thread::spawn(move || decode_worker(receive, state));
        let (reply, result) = bounded(1);
        send.send(Command::Tracks(
            Some(PathBuf::from("/nonexistent/voice-audio-test.wav")),
            None,
            1,
            reply,
        ))
        .unwrap();
        assert!(
            result
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .is_err()
        );
        send.send(Command::Shutdown).unwrap();
        worker.join().unwrap();
    }
    #[test]
    fn output_replaces_nonfinite_samples_with_silence_and_limits_clipping() {
        assert_eq!(safe_output(f32::NAN), 0.0);
        assert_eq!(safe_output(f32::INFINITY), 0.0);
        assert_eq!(safe_output(1.5), 1.0);
        assert_eq!(safe_output(-1.5), -1.0);
    }
    #[test]
    #[ignore = "Manual output-only hardware diagnostic; select VOICE_AUDIO_PROBE_OUTPUT explicitly"]
    fn native_selected_output_probe() {
        let name = std::env::var("VOICE_AUDIO_PROBE_OUTPUT")
            .expect("Set VOICE_AUDIO_PROBE_OUTPUT to an output device name");
        eprintln!(
            "probe inventory={:?}",
            list_device_names()
                .unwrap()
                .outputs
                .iter()
                .map(|d| &d.name)
                .collect::<Vec<_>>()
        );
        let mut engine = AudioEngine::unconfigured().unwrap();
        eprintln!("probe default config query output={name:?}");
        let device = select_device(Some(&name), false).unwrap();
        eprintln!(
            "probe default config={:?}",
            device.default_output_config().unwrap()
        );
        eprintln!("probe configuring output={name:?}");
        engine
            .configure_audio(&AudioPreferences {
                output_device: Some(name),
                ..Default::default()
            })
            .unwrap();
        thread::sleep(Duration::from_secs(1));
        eprintln!("probe configured diagnostics={:?}", engine.diagnostics());
        drop(engine);
        eprintln!("probe selected output closed");
    }
    #[test]
    fn diagnostic_delay_tracks_live_input_rate_and_compensation_then_clears_on_stop() {
        let shared = Arc::new(Shared::new());
        let handle = AudioHandle {
            shared: shared.clone(),
        };
        let mut diagnostics = AudioDiagnostics {
            input_sample_rate: 1,
            ..Default::default()
        };
        handle.refresh_diagnostics(&mut diagnostics);
        assert_eq!(diagnostics.analysis_delay_seconds, 0.0);
        shared.input_active.store(true, Ordering::Release);
        shared.input_rate.store(48_000, Ordering::Release);
        shared.input_frames.store(256, Ordering::Relaxed);
        shared.input_drops.store(7, Ordering::Relaxed);
        store_f64(&shared.output_lead, 0.01);
        handle.refresh_diagnostics(&mut diagnostics);
        let before = diagnostics.analysis_delay_seconds;
        assert!((before - (0.01 + 3328.0 / 48_000.0 + 0.005)).abs() < 1e-9);
        assert_eq!(diagnostics.input_buffer_frames, 256);
        assert_eq!(diagnostics.input_drops, 7);
        handle.set_latency_compensation(100.0);
        handle.refresh_diagnostics(&mut diagnostics);
        assert!((diagnostics.analysis_delay_seconds - before - 0.1).abs() < 1e-9);
        handle.stop_input_capture();
        handle.refresh_diagnostics(&mut diagnostics);
        assert_eq!(diagnostics.analysis_delay_seconds, 0.0);
    }
}
