//! Native, bounded-memory vocal analysis. Raw PCM is never retained for a whole file.
//! The Basic Pitch note decoder is adapted from Spotify basic-pitch, Copyright 2022 Spotify AB,
//! under Apache-2.0; see assets/models/basic-pitch/LICENSE and PROVENANCE.json.
use crate::media::AudioReader;
use crate::model::{AnalysisMode, LinePolicy, Note, PitchFrame, TrackAnalysis, WaveformPeak};
use anyhow::{Context, Result, bail};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub const ANALYSIS_RATE: u32 = 16_000;
pub const ANALYSIS_HOP: f64 = 0.025;
pub const MIN_FREQUENCY: f32 = 65.0;
pub const MAX_FREQUENCY: f32 = 1_200.0;
const WINDOW: usize = 1024;

pub fn frequency_to_midi(frequency: f32) -> f32 {
    if frequency > 0.0 {
        69.0 + 12.0 * (frequency / 440.0).log2()
    } else {
        0.0
    }
}
pub fn midi_to_frequency(midi: f32) -> f32 {
    440.0 * 2.0f32.powf((midi - 69.0) / 12.0)
}

/// YIN baseline. This allocates scratch memory: run it on an analysis thread, not an audio callback.
pub fn detect_pitch(samples: &[f32], sample_rate: u32, time: f64) -> PitchFrame {
    let mean = samples.iter().copied().sum::<f32>() / samples.len().max(1) as f32;
    let rms = (samples.iter().map(|x| (*x - mean).powi(2)).sum::<f32>()
        / samples.len().max(1) as f32)
        .sqrt();
    let empty = PitchFrame {
        time,
        rms,
        ..PitchFrame::default()
    };
    if samples.len() < 128 || !rms.is_finite() || rms < 0.006 || sample_rate == 0 {
        return empty;
    }
    let half = samples.len() / 2;
    let min_lag = ((sample_rate as f32 / MAX_FREQUENCY).floor() as usize).max(2);
    let max_lag = (half - 1).min((sample_rate as f32 / MIN_FREQUENCY).ceil() as usize);
    if max_lag <= min_lag {
        return empty;
    }
    let mut difference = vec![0.0f32; max_lag + 1];
    for lag in 1..=max_lag {
        difference[lag] = (0..half)
            .map(|j| (samples[j] - samples[j + lag]).powi(2))
            .sum();
    }
    let mut running = 0.0;
    difference[0] = 1.0;
    for (lag, value) in difference.iter_mut().enumerate().skip(1) {
        running += *value;
        *value = if running > 0.0 {
            *value * lag as f32 / running
        } else {
            1.0
        };
    }
    let mut lag = min_lag;
    while lag < max_lag {
        if difference[lag] < 0.15 {
            while lag < max_lag && difference[lag + 1] < difference[lag] {
                lag += 1;
            }
            break;
        }
        lag += 1;
    }
    if lag >= max_lag {
        lag = (min_lag..=max_lag)
            .min_by(|a, b| difference[*a].total_cmp(&difference[*b]))
            .unwrap();
        if difference[lag] > 0.15 {
            return empty;
        }
    }
    let confidence = (1.0 - difference[lag]).max(0.0);
    if confidence < 0.82 {
        return empty;
    }
    let mut refined = lag as f32;
    if lag > 1 && lag < max_lag {
        let denominator = difference[lag - 1] - 2.0 * difference[lag] + difference[lag + 1];
        if denominator.abs() > 1e-9 {
            refined += (difference[lag - 1] - difference[lag + 1]) / (2.0 * denominator);
        }
    }
    let frequency = sample_rate as f32 / refined;
    if !(MIN_FREQUENCY..=MAX_FREQUENCY).contains(&frequency) {
        return empty;
    }
    PitchFrame {
        time,
        frequency,
        midi: frequency_to_midi(frequency),
        confidence,
        rms,
    }
}

/// Streaming box integration uses the same weighting rule as the historical web resampler.
struct BoxResampler {
    pending: Vec<f32>,
    position: f64,
    ratio: f64,
}
impl BoxResampler {
    fn new(source: u32, target: u32) -> Self {
        Self {
            pending: Vec::new(),
            position: 0.0,
            ratio: source as f64 / target as f64,
        }
    }
    fn push(&mut self, samples: &[f32]) -> Vec<f32> {
        self.pending.extend(
            samples
                .iter()
                .map(|sample| if sample.is_finite() { *sample } else { 0.0 }),
        );
        let mut output =
            Vec::with_capacity((samples.len() as f64 / self.ratio).ceil() as usize + 1);
        while self.position + self.ratio <= self.pending.len() as f64 + 1e-9 {
            let end = self.position + self.ratio;
            let mut sum = 0.0f64;
            for i in self.position.floor() as usize..end.ceil() as usize {
                let weight = ((i + 1) as f64).min(end) - (i as f64).max(self.position);
                sum += self.pending.get(i).copied().unwrap_or(0.0) as f64 * weight.max(0.0);
            }
            output.push((sum / self.ratio) as f32);
            self.position = end;
        }
        let consumed = (self.position.floor() as usize).min(self.pending.len());
        self.pending.drain(..consumed);
        self.position -= consumed as f64;
        output
    }
}

struct WaveformBuilder {
    peaks: Vec<WaveformPeak>,
    bucket: usize,
    in_bucket: usize,
    frame: u64,
    start: u64,
    min: f32,
    max: f32,
    rate: u32,
}
impl WaveformBuilder {
    fn new(rate: u32) -> Self {
        Self {
            peaks: Vec::new(),
            bucket: (rate as usize / 200).max(1),
            in_bucket: 0,
            frame: 0,
            start: 0,
            min: 0.0,
            max: 0.0,
            rate,
        }
    }
    fn push(&mut self, samples: &[f32]) {
        for sample in samples {
            self.min = self.min.min(*sample);
            self.max = self.max.max(*sample);
            self.frame += 1;
            self.in_bucket += 1;
            if self.in_bucket >= self.bucket {
                self.flush();
            }
        }
    }
    fn flush(&mut self) {
        if self.in_bucket == 0 {
            return;
        }
        self.peaks.push(WaveformPeak {
            time: self.start as f64 / self.rate as f64,
            min: self.min,
            max: self.max,
        });
        self.start = self.frame;
        self.in_bucket = 0;
        self.min = 0.0;
        self.max = 0.0;
        if self.peaks.len() >= 4096 {
            self.peaks = self
                .peaks
                .chunks(2)
                .map(|pair| WaveformPeak {
                    time: pair[0].time,
                    min: pair.iter().map(|p| p.min).fold(0.0, f32::min),
                    max: pair.iter().map(|p| p.max).fold(0.0, f32::max),
                })
                .collect();
            self.bucket *= 2;
        }
    }
    fn finish(mut self) -> Vec<WaveformPeak> {
        self.flush();
        self.peaks
    }
}

pub fn segment_notes(frames: &[PitchFrame], duration: f64) -> Vec<Note> {
    let mut notes = Vec::new();
    let mut active: Option<Note> = None;
    for frame in frames {
        if frame.frequency > 0.0 && frame.confidence >= 0.82 && frame.midi.is_finite() {
            let midi = frame.midi.round();
            let start = (frame.time - ANALYSIS_HOP / 2.0).max(0.0);
            let end = (frame.time + ANALYSIS_HOP / 2.0).min(duration);
            if let Some(note) = active
                .as_mut()
                .filter(|n| n.midi == midi && start - n.end < ANALYSIS_HOP * 1.6)
            {
                note.end = end;
                note.confidence = note.confidence.min(frame.confidence);
            } else {
                if let Some(note) = active.take().filter(|n| n.end - n.start >= 0.07) {
                    notes.push(note);
                }
                active = Some(Note {
                    start,
                    end,
                    midi,
                    confidence: frame.confidence,
                });
            }
        } else if let Some(note) = active.take().filter(|n| n.end - n.start >= 0.07) {
            notes.push(note);
        }
    }
    if let Some(note) = active.filter(|n| n.end - n.start >= 0.07) {
        notes.push(note);
    }
    notes
}

fn smooth_frames(frames: &mut [PitchFrame]) {
    let original: Vec<Option<f32>> = frames
        .iter()
        .map(|p| (p.frequency > 0.0).then_some(p.midi))
        .collect();
    for i in 1..frames.len().saturating_sub(1) {
        if let (Some(a), Some(b), Some(c)) = (original[i - 1], original[i], original[i + 1]) {
            let mut values = [a, b, c];
            values.sort_by(f32::total_cmp);
            frames[i].midi = values[1];
            frames[i].frequency = midi_to_frequency(values[1]);
        }
    }
}

fn analyze_mono<F: Fn(f32)>(
    path: &Path,
    cancel: &AtomicBool,
    progress: &F,
) -> Result<TrackAnalysis> {
    let mut reader = AudioReader::open(path)?;
    let metadata = reader.metadata();
    let mut resampler = BoxResampler::new(metadata.sample_rate, ANALYSIS_RATE);
    let mut waveform = WaveformBuilder::new(metadata.sample_rate);
    let mut pending = VecDeque::<f32>::new();
    let mut pending_start = 0usize;
    let mut produced = 0usize;
    let mut next_center = 0usize;
    let mut source_frames = 0u64;
    let mut frames = Vec::new();
    let mut window = vec![0.0; WINDOW];
    loop {
        if cancel.load(Ordering::Relaxed) {
            bail!("Analysis cancelled");
        }
        let decoded = reader.read_mono(metadata.sample_rate as usize)?;
        let eof = decoded.is_empty();
        source_frames += decoded.len() as u64;
        waveform.push(&decoded);
        let resampled = resampler.push(&decoded);
        produced += resampled.len();
        pending.extend(resampled);
        while next_center < produced && (eof || next_center + WINDOW / 2 <= produced) {
            window.fill(0.0);
            for (i, sample) in window.iter_mut().enumerate() {
                let index = next_center as i64 + i as i64 - (WINDOW / 2) as i64;
                if index >= pending_start as i64 && index < produced as i64 {
                    *sample = pending[(index as usize) - pending_start];
                }
            }
            frames.push(detect_pitch(
                &window,
                ANALYSIS_RATE,
                next_center as f64 / ANALYSIS_RATE as f64,
            ));
            next_center += (ANALYSIS_RATE as f64 * ANALYSIS_HOP).round() as usize;
        }
        let discard_until = next_center.saturating_sub(WINDOW / 2).min(produced);
        let discard = discard_until
            .saturating_sub(pending_start)
            .min(pending.len());
        pending.drain(..discard);
        pending_start += discard;
        progress(if eof {
            1.0
        } else if metadata.duration > 0.0 {
            ((source_frames as f64 / metadata.sample_rate as f64 / metadata.duration) as f32)
                .min(0.99)
        } else {
            0.0
        });
        if eof {
            break;
        }
    }
    smooth_frames(&mut frames);
    let duration = source_frames as f64 / metadata.sample_rate as f64;
    let notes = segment_notes(&frames, duration);
    Ok(TrackAnalysis {
        version: "native-yin-1".into(),
        mode: AnalysisMode::Monophonic,
        frames,
        notes,
        waveform: waveform.finish(),
        reviewed: true,
        ..TrackAnalysis::default()
    })
}

pub fn analyze_file<F: Fn(f32)>(
    path: &Path,
    mode: AnalysisMode,
    cancel: Arc<AtomicBool>,
    progress: F,
) -> Result<TrackAnalysis> {
    if cancel.load(Ordering::Relaxed) {
        bail!("Analysis cancelled");
    }
    match mode {
        AnalysisMode::Monophonic => analyze_mono(path, &cancel, &progress),
        AnalysisMode::Polyphonic => analyze_polyphonic(path, &cancel, &progress),
    }
    .with_context(|| format!("Could not analyze {}", path.display()))
}

/// Select a register-constrained provisional line; this does not identify a singer or voice part.
pub fn selected_notes(analysis: &TrackAnalysis) -> Vec<Note> {
    if analysis.mode == AnalysisMode::Monophonic {
        return analysis.notes.clone();
    }
    let candidates: Vec<Note> = analysis
        .notes
        .iter()
        .copied()
        .filter(|n| {
            n.midi >= analysis.selection.min_midi as f32
                && n.midi <= analysis.selection.max_midi as f32
                && n.start.is_finite()
                && n.end > n.start
        })
        .collect();
    let mut events: Vec<(f64, bool, usize)> = candidates
        .iter()
        .enumerate()
        .flat_map(|(i, n)| [(n.start, true, i), (n.end, false, i)])
        .collect();
    events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut active = std::collections::BTreeMap::<i16, std::collections::BTreeSet<usize>>::new();
    let mut selected: Vec<Note> = Vec::new();
    let mut cursor = 0;
    while cursor < events.len() {
        let time = events[cursor].0;
        while cursor < events.len() && (events[cursor].0 - time).abs() < 1e-8 {
            let (_, starting, index) = events[cursor];
            let key = candidates[index].midi.round() as i16;
            if starting {
                active.entry(key).or_default().insert(index);
            } else if let Some(indices) = active.get_mut(&key) {
                indices.remove(&index);
                if indices.is_empty() {
                    active.remove(&key);
                }
            }
            cursor += 1;
        }
        let Some(next) = events.get(cursor).map(|e| e.0) else {
            break;
        };
        let choice = match analysis.selection.policy {
            LinePolicy::Lowest => active.first_key_value(),
            LinePolicy::Highest => active.last_key_value(),
        };
        if let Some((_, indices)) = choice {
            let chosen = candidates[*indices.first().unwrap()];
            if let Some(last) = selected
                .last_mut()
                .filter(|last| last.midi == chosen.midi && (last.end - time).abs() < 1e-6)
            {
                last.end = next;
                last.confidence = last.confidence.min(chosen.confidence);
            } else {
                selected.push(Note {
                    start: time,
                    end: next,
                    ..chosen
                });
            }
        }
    }
    selected
}

pub fn practice_target(analysis: &TrackAnalysis) -> TrackAnalysis {
    if analysis.mode == AnalysisMode::Monophonic {
        return analysis.clone();
    }
    let mut target = analysis.clone();
    target.notes = selected_notes(analysis);
    target.frames.clear();
    let duration = target.notes.iter().map(|n| n.end).fold(0.0, f64::max);
    let mut note_index = 0;
    for index in 0..=(duration / ANALYSIS_HOP).ceil() as usize {
        let time = index as f64 * ANALYSIS_HOP;
        while note_index < target.notes.len() && target.notes[note_index].end <= time {
            note_index += 1;
        }
        let active = target
            .notes
            .get(note_index)
            .filter(|n| n.start <= time && n.end > time);
        target.frames.push(if let Some(n) = active {
            PitchFrame {
                time,
                frequency: midi_to_frequency(n.midi),
                midi: n.midi,
                confidence: n.confidence.max(0.82),
                rms: 0.0,
            }
        } else {
            PitchFrame {
                time,
                ..PitchFrame::default()
            }
        });
    }
    target
}

const POLY_RATE: u32 = 22_050;
const POLY_WINDOW: usize = 43_844;
const POLY_HOP: usize = 36_164;
const POLY_OVERLAP_HALF: usize = 15;
const POLY_KEYS: usize = 88;

struct NativePolyModel {
    session: ort::session::Session,
}
impl NativePolyModel {
    fn new() -> Result<Self> {
        let session = ort::session::Session::builder()?
            .with_intra_threads(2)?
            .commit_from_memory(include_bytes!("../assets/models/basic-pitch/nmp.onnx"))?;
        Ok(Self { session })
    }
    fn analyze(&mut self, samples: &[f32], cancel: &AtomicBool) -> Result<Vec<Note>> {
        let mut padded = vec![0.0; 3840];
        padded.extend_from_slice(samples);
        let mut frames: Vec<[f32; POLY_KEYS]> = Vec::new();
        let mut onsets: Vec<[f32; POLY_KEYS]> = Vec::new();
        let expected = (samples.len() as f64 / POLY_HOP as f64 * 142.0).floor() as usize;
        for start in (0..padded.len()).step_by(POLY_HOP) {
            if cancel.load(Ordering::Relaxed) {
                bail!("Analysis cancelled");
            }
            let mut window = vec![0.0; POLY_WINDOW];
            let count = POLY_WINDOW.min(padded.len() - start);
            window[..count].copy_from_slice(&padded[start..start + count]);
            let input = ort::value::Tensor::from_array((
                vec![1usize, POLY_WINDOW, 1],
                window.into_boxed_slice(),
            ))?;
            let output = self
                .session
                .run(ort::inputs!["serving_default_input_2:0" => input])?;
            let (frame_shape, frame_values) =
                output["StatefulPartitionedCall:1"].try_extract_tensor::<f32>()?;
            let (onset_shape, onset_values) =
                output["StatefulPartitionedCall:2"].try_extract_tensor::<f32>()?;
            anyhow::ensure!(
                frame_shape.len() == 3
                    && frame_shape[2] == POLY_KEYS as i64
                    && frame_shape == onset_shape,
                "Unexpected Basic Pitch output shape"
            );
            let rows = frame_shape[1] as usize;
            for row in POLY_OVERLAP_HALF..rows.saturating_sub(POLY_OVERLAP_HALF) {
                if frames.len() >= expected {
                    break;
                }
                frames.push(std::array::from_fn(|key| {
                    let value = frame_values[row * POLY_KEYS + key];
                    if value.is_finite() {
                        value.clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                }));
                onsets.push(std::array::from_fn(|key| {
                    let value = onset_values[row * POLY_KEYS + key];
                    if value.is_finite() {
                        value.clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                }));
            }
        }
        Ok(poly_notes(&frames, &onsets))
    }
}

fn poly_frame_time(frame: usize) -> f64 {
    let offset = 256.0 / POLY_RATE as f64 * (172.0 - POLY_WINDOW as f64 / 256.0) + 0.0018;
    frame as f64 * 256.0 / POLY_RATE as f64 - offset * (frame / 172) as f64
}

/// Native port of Spotify's onset/remaining-energy note decoder (Apache-2.0).
/// Outputs note centres, not singer identities or continuously estimated vocal F0.
fn poly_notes(frames: &[[f32; POLY_KEYS]], onsets: &[[f32; POLY_KEYS]]) -> Vec<Note> {
    let count = frames.len();
    if count < 3 {
        return Vec::new();
    }
    let mut inferred = onsets.to_vec();
    let mut differences = vec![[0.0; POLY_KEYS]; count];
    let mut max_difference = 0.0f32;
    let max_onset = onsets
        .iter()
        .flat_map(|r| r.iter())
        .copied()
        .fold(0.0, f32::max);
    for time in 2..count {
        for pitch in 0..POLY_KEYS {
            let difference = (frames[time][pitch] - frames[time - 1][pitch])
                .min(frames[time][pitch] - frames[time - 2][pitch])
                .max(0.0);
            differences[time][pitch] = difference;
            max_difference = max_difference.max(difference);
        }
    }
    if max_difference > 0.0 {
        for time in 0..count {
            for pitch in 0..POLY_KEYS {
                inferred[time][pitch] = inferred[time][pitch]
                    .max(differences[time][pitch] * max_onset / max_difference);
            }
        }
    }
    let mut peaks = Vec::new();
    for time in 1..count - 1 {
        for pitch in 0..POLY_KEYS {
            if inferred[time][pitch] >= 0.5
                && inferred[time][pitch] > inferred[time - 1][pitch]
                && inferred[time][pitch] > inferred[time + 1][pitch]
            {
                peaks.push((time, pitch));
            }
        }
    }
    let mut remaining = frames.to_vec();
    let mut events = Vec::<(usize, usize, usize)>::new();
    let clear = |energy: &mut Vec<[f32; POLY_KEYS]>, time: usize, pitch: usize| {
        energy[time][pitch.saturating_sub(1)..=(pitch + 1).min(POLY_KEYS - 1)].fill(0.0);
    };
    for (start, pitch) in peaks.into_iter().rev() {
        let mut end = start + 1;
        let mut gap = 0;
        while end < count - 1 && gap < 11 {
            if remaining[end][pitch] < 0.3 {
                gap += 1;
            } else {
                gap = 0;
            }
            end += 1;
        }
        end = end.saturating_sub(gap);
        if end.saturating_sub(start) <= 5 {
            continue;
        }
        for time in start..end {
            clear(&mut remaining, time, pitch);
        }
        events.push((start, end, pitch));
    }
    // The energy fallback retains sustained notes without a strong predicted onset.
    loop {
        let maximum = remaining
            .iter()
            .enumerate()
            .flat_map(|(t, row)| row.iter().enumerate().map(move |(p, v)| (t, p, *v)))
            .max_by(|a, b| a.2.total_cmp(&b.2));
        let Some((middle, pitch, value)) = maximum else {
            break;
        };
        if value <= 0.3 {
            break;
        }
        remaining[middle][pitch] = 0.0;
        let mut end = middle + 1;
        let mut gap = 0;
        while end < count - 1 && gap < 11 {
            if remaining[end][pitch] < 0.3 {
                gap += 1;
            } else {
                gap = 0;
            }
            clear(&mut remaining, end, pitch);
            end += 1;
        }
        let end = end.saturating_sub(1 + gap);
        let mut start = middle as isize - 1;
        gap = 0;
        while start > 0 && gap < 11 {
            if remaining[start as usize][pitch] < 0.3 {
                gap += 1;
            } else {
                gap = 0;
            }
            clear(&mut remaining, start as usize, pitch);
            start -= 1;
        }
        let start = (start + 1 + gap as isize).max(0) as usize;
        if end.saturating_sub(start) > 5 {
            events.push((start, end, pitch));
        }
    }
    let mut notes: Vec<Note> = events
        .into_iter()
        .map(|(start, end, pitch)| Note {
            start: poly_frame_time(start),
            end: poly_frame_time(end),
            midi: (pitch + 21) as f32,
            confidence: frames[start..end].iter().map(|r| r[pitch]).sum::<f32>()
                / (end - start) as f32,
        })
        .filter(|note| note.end > note.start)
        .collect();
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.midi.total_cmp(&b.midi)));
    notes
}

fn analyze_polyphonic<F: Fn(f32)>(
    path: &Path,
    cancel: &AtomicBool,
    progress: &F,
) -> Result<TrackAnalysis> {
    let mut reader = AudioReader::open(path)?;
    let metadata = reader.metadata();
    let mut waveform = WaveformBuilder::new(metadata.sample_rate);
    let mut decoded_frames = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            bail!("Analysis cancelled");
        }
        let samples = reader.read_mono(metadata.sample_rate as usize)?;
        if samples.is_empty() {
            break;
        }
        decoded_frames += samples.len() as u64;
        waveform.push(&samples);
        if metadata.duration > 0.0 {
            progress(
                (decoded_frames as f64 / metadata.sample_rate as f64 / metadata.duration * 0.1)
                    .min(0.1) as f32,
            );
        }
    }
    let duration = decoded_frames as f64 / metadata.sample_rate as f64;
    let mut model =
        NativePolyModel::new().context("Could not load the bundled native Basic Pitch model")?;
    let mut candidates: Vec<(Note, bool, bool)> = Vec::new();
    let mut core_start = 0.0f64;
    while core_start < duration {
        if cancel.load(Ordering::Relaxed) {
            bail!("Analysis cancelled");
        }
        let core_end = (core_start + 20.0).min(duration);
        let start = (core_start - 0.5).max(0.0);
        let end = (core_end + 0.5).min(duration);
        reader.seek(start)?;
        let wanted = ((end - start) * metadata.sample_rate as f64).round() as usize;
        let mut consumed = 0;
        let mut resampler = BoxResampler::new(metadata.sample_rate, POLY_RATE);
        let mut mono = Vec::with_capacity(((end - start) * POLY_RATE as f64).ceil() as usize);
        while consumed < wanted {
            if cancel.load(Ordering::Relaxed) {
                bail!("Analysis cancelled");
            }
            let samples =
                reader.read_mono((wanted - consumed).min(metadata.sample_rate as usize))?;
            if samples.is_empty() {
                break;
            }
            consumed += samples.len();
            mono.extend(resampler.push(&samples));
        }
        for local in model.analyze(&mono, cancel)? {
            let absolute_start = start + local.start;
            let absolute_end = start + local.end;
            let note = Note {
                start: absolute_start.max(core_start),
                end: absolute_end.min(core_end),
                ..local
            };
            if note.end - note.start >= 0.025 {
                candidates.push((
                    note,
                    absolute_start < core_start - 0.025,
                    absolute_end > core_end + 0.025,
                ));
            }
        }
        core_start = core_end;
        progress((0.1 + 0.9 * core_end / duration.max(0.001)) as f32);
    }
    candidates.sort_by(|a, b| {
        a.0.midi
            .total_cmp(&b.0.midi)
            .then(a.0.start.total_cmp(&b.0.start))
    });
    let mut merged: Vec<(Note, bool, bool)> = Vec::new();
    for (note, left, right) in candidates {
        if let Some((last, _, last_right)) = merged.last_mut().filter(|(last, _, last_right)| {
            last.midi == note.midi && *last_right && left && (last.end - note.start).abs() < 0.002
        }) {
            last.end = note.end;
            *last_right = right;
        } else {
            merged.push((note, left, right));
        }
    }
    let mut notes: Vec<Note> = merged.into_iter().map(|(note, _, _)| note).collect();
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.midi.total_cmp(&b.midi)));
    progress(1.0);
    Ok(TrackAnalysis {
        version: "native-basic-pitch-onnx-1".into(),
        mode: AnalysisMode::Polyphonic,
        notes,
        waveform: waveform.finish(),
        reviewed: false,
        ..TrackAnalysis::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tone(frequency: f32, rate: u32, count: usize) -> Vec<f32> {
        (0..count)
            .map(|i| (std::f32::consts::TAU * frequency * i as f32 / rate as f32).sin() * 0.25)
            .collect()
    }
    #[test]
    fn yin_controlled_tones_and_silence() {
        for frequency in [82.41, 110.0, 220.0, 440.0, 880.0] {
            let p = detect_pitch(&tone(frequency, 16000, 1024), 16000, 1.25);
            assert!(p.voiced(), "{frequency}: {p:?}");
            assert!(
                (1200.0 * (p.frequency / frequency).log2()).abs() < 12.0,
                "{frequency}: {p:?}"
            );
            assert_eq!(p.time, 1.25);
        }
        assert!(!detect_pitch(&vec![0.0; 1024], 16000, 0.0).voiced());
    }
    #[test]
    fn resampling_is_invariant_to_decode_chunk_boundaries() {
        let source = tone(220.0, 44100, 44100);
        let mut whole = BoxResampler::new(44100, 16000);
        let expected = whole.push(&source);
        let mut stream = BoxResampler::new(44100, 16000);
        let actual: Vec<f32> = source.chunks(997).flat_map(|s| stream.push(s)).collect();
        assert_eq!(actual.len(), expected.len());
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(a, b)| (*a - b).abs() < 1e-5)
        );
    }
    #[test]
    fn waveform_memory_is_bounded() {
        let mut wave = WaveformBuilder::new(100);
        wave.push(&vec![0.3; 1_000_000]);
        let result = wave.finish();
        assert!(result.len() <= 4096);
        assert_eq!(result[0].max, 0.3);
    }
    #[test]
    fn provisional_selection_preserves_the_review_gate() {
        let analysis = TrackAnalysis {
            mode: AnalysisMode::Polyphonic,
            notes: vec![
                Note {
                    start: 0.0,
                    end: 1.0,
                    midi: 60.0,
                    confidence: 0.9,
                },
                Note {
                    start: 0.0,
                    end: 1.0,
                    midi: 67.0,
                    confidence: 0.9,
                },
            ],
            ..TrackAnalysis::default()
        };
        let selected = practice_target(&analysis);
        assert!(!selected.reviewed);
        assert_eq!(selected.notes.len(), 1);
        assert_eq!(selected.notes[0].midi, 67.0);
        let mut low = analysis;
        low.selection.policy = LinePolicy::Lowest;
        assert_eq!(selected_notes(&low)[0].midi, 60.0);
    }
    #[test]
    fn bundled_onnx_runs_native_inference() {
        let mut model = NativePolyModel::new().expect("Bundled native ONNX model must load");
        let notes = model
            .analyze(
                &tone(440.0, POLY_RATE, POLY_RATE as usize * 2),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert!(
            notes.iter().any(|note| (note.midi - 69.0).abs() < 0.1),
            "Expected A4 candidate, got {notes:?}"
        );
    }
    #[test]
    fn bundled_onnx_preserves_simultaneous_candidates() {
        let mut model = NativePolyModel::new().unwrap();
        let mut mixed = vec![0.0; POLY_RATE as usize * 2];
        for frequency in [440.0, 554.365, 659.255] {
            for (sample, value) in
                mixed
                    .iter_mut()
                    .zip(tone(frequency, POLY_RATE, POLY_RATE as usize * 2))
            {
                *sample += value / 3.0;
            }
        }
        let notes = model.analyze(&mixed, &AtomicBool::new(false)).unwrap();
        let expected = [69.0f32, 73.0, 76.0]
            .iter()
            .filter(|midi| notes.iter().any(|note| (note.midi - **midi).abs() < 0.1))
            .count();
        assert!(
            expected >= 2,
            "Expected multiple chord candidates: {notes:?}"
        );
    }
    #[test]
    fn cancelled_analysis_returns_before_opening_media() {
        assert!(
            analyze_file(
                Path::new("missing.wav"),
                AnalysisMode::Monophonic,
                Arc::new(AtomicBool::new(true)),
                |_| {}
            )
            .unwrap_err()
            .to_string()
            .contains("cancelled")
        );
    }
    #[test]
    #[ignore = "Requires a privately cached real recording; no audio is distributed"]
    fn real_polyphonic_pipeline_smoke() {
        let path = std::env::var("VOCADITO_CLIP").expect("Set VOCADITO_CLIP to a private WAV path");
        let analysis = analyze_file(
            Path::new(&path),
            AnalysisMode::Polyphonic,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .unwrap();
        assert!(!analysis.notes.is_empty());
        assert!(!analysis.reviewed);
        assert!(analysis.waveform.len() <= 4096);
        assert!(
            analysis
                .notes
                .windows(2)
                .all(|pair| pair[0].start <= pair[1].start)
        );
        println!(
            "native real-recording polyphonic pipeline smoke: {} candidate notes, review still required",
            analysis.notes.len()
        );
    }
    #[test]
    #[ignore = "Requires the privately cached Vocadito corpus; no audio is distributed"]
    fn real_vocadito_clip_smoke() {
        let path = std::env::var("VOCADITO_CLIP").expect("Set VOCADITO_CLIP to a private WAV path");
        let analysis = analyze_file(
            Path::new(&path),
            AnalysisMode::Monophonic,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .unwrap();
        assert!(!analysis.frames.is_empty());
        assert!(analysis.frames.iter().any(|frame| frame.voiced()));
        assert!(analysis.waveform.len() <= 4096);
        println!(
            "native real-clip smoke: {} pitch frames, {} notes",
            analysis.frames.len(),
            analysis.notes.len()
        );
    }
}
