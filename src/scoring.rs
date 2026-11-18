//! Duration-based scoring with explicit transport segments and a capture tail.
use crate::model::{PitchFrame, PracticeTake, new_id, timestamp_ms};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default)]
pub struct ScoreSnapshot {
    pub score: f32,
    pub coverage: f32,
    pub cents: Option<f32>,
    pub target_midi: Option<f32>,
    pub matched_seconds: f64,
    pub target_seconds: f64,
}

pub fn capture_evaluation_delay(window_size: usize, sample_rate: u32, compensation_ms: f32) -> f64 {
    window_size as f64 / (2.0 * sample_rate.max(1) as f64)
        + compensation_ms.max(0.0) as f64 / 1000.0
        + 0.08
}
pub fn evaluated_position(position: f64, attempt_start: f64, delay: f64) -> f64 {
    attempt_start.max(position - delay.max(0.0))
}
pub fn pitch_at(frames: &[PitchFrame], time: f64) -> Option<PitchFrame> {
    if !time.is_finite() || frames.is_empty() {
        return None;
    }
    let index = frames.partition_point(|f| f.time < time);
    let before = index.checked_sub(1).and_then(|i| frames.get(i));
    let after = frames.get(index);
    let frame = match (before, after) {
        (Some(a), Some(b)) => {
            if (b.time - time).abs() < (a.time - time).abs() {
                *b
            } else {
                *a
            }
        }
        (Some(a), None) => *a,
        (None, Some(b)) => *b,
        _ => return None,
    };
    (frame.voiced() && (frame.time - time).abs() <= 0.065).then_some(frame)
}

#[derive(Default)]
pub struct PracticeScorer {
    started_at_ms: u64,
    bins: BTreeMap<i64, u8>,
    hits: usize,
    sung: usize,
    cursor: Option<f64>,
    segment_start: f64,
    previous_sample: Option<f64>,
    cents: Option<f32>,
    target_midi: Option<f32>,
}
impl PracticeScorer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Resume/seek must start a segment, so missing time between segments is never invented.
    pub fn begin_segment(&mut self, time: f64) {
        if time.is_finite() {
            if self.started_at_ms == 0 {
                self.started_at_ms = timestamp_ms();
            }
            self.cursor = Some(time.max(0.0));
            self.segment_start = time.max(0.0);
            self.previous_sample = None;
        }
    }
    fn write(&mut self, bin: i64, value: u8) {
        let old = self.bins.insert(bin, value).unwrap_or(0);
        self.hits = self.hits + usize::from(value == 3) - usize::from(old == 3);
        self.sung = self.sung + usize::from(value > 0) - usize::from(old > 0);
    }
    pub fn snapshot(&self) -> ScoreSnapshot {
        let n = self.bins.len();
        ScoreSnapshot {
            score: if n > 0 {
                100.0 * self.hits as f32 / n as f32
            } else {
                0.0
            },
            coverage: if n > 0 {
                100.0 * self.sung as f32 / n as f32
            } else {
                0.0
            },
            cents: self.cents,
            target_midi: self.target_midi,
            matched_seconds: self.hits as f64 * 0.02,
            target_seconds: n as f64 * 0.02,
        }
    }
    /// The transport supplies missing-voice bins; target silence never enters the denominator.
    pub fn advance(&mut self, frames: &[PitchFrame], time: f64) -> ScoreSnapshot {
        if !time.is_finite() {
            return self.snapshot();
        }
        if let Some(cursor) = self.cursor.filter(|cursor| time >= *cursor) {
            for bin in (cursor / 0.02).floor() as i64..=(time / 0.02).floor() as i64 {
                if bin as f64 * 0.02 + 1e-8 >= self.segment_start
                    && !self.bins.contains_key(&bin)
                    && pitch_at(frames, bin as f64 * 0.02).is_some()
                {
                    self.write(bin, 0);
                }
            }
        }
        self.cursor = Some(time);
        self.snapshot()
    }
    pub fn sample(
        &mut self,
        frames: &[PitchFrame],
        time: f64,
        voice: PitchFrame,
        tolerance: f32,
    ) -> ScoreSnapshot {
        if !time.is_finite() || time < self.segment_start || time < 0.0 {
            return self.snapshot();
        }
        let target = pitch_at(frames, time);
        let voiced = voice.voiced();
        self.cents = target
            .filter(|_| voiced)
            .map(|target| (voice.midi - target.midi) * 100.0);
        self.target_midi = target.map(|target| target.midi);
        let end = (time / 0.02).floor() as i64;
        let start = self
            .previous_sample
            .filter(|previous| *previous <= time)
            .map_or(end, |previous| {
                end.min((previous / 0.02).floor() as i64 + 1)
                    .max(((time - 0.06) / 0.02).ceil() as i64)
            });
        for bin in start..=end {
            let at = bin as f64 * 0.02;
            if at + 1e-8 < self.segment_start {
                continue;
            }
            if let Some(reference) = pitch_at(frames, at) {
                let hit = voiced
                    && ((voice.midi - reference.midi) * 100.0).abs() <= tolerance.clamp(1.0, 200.0);
                self.write(
                    bin,
                    if hit {
                        3
                    } else if voiced {
                        1
                    } else {
                        0
                    },
                );
            }
        }
        self.previous_sample = Some(time);
        self.snapshot()
    }
    pub fn to_take(
        &self,
        target_id: &str,
        start: f64,
        end: f64,
        trace: Vec<PitchFrame>,
    ) -> PracticeTake {
        let score = self.snapshot();
        PracticeTake {
            id: new_id(),
            started_at_ms: if self.started_at_ms > 0 {
                self.started_at_ms
            } else {
                timestamp_ms()
            },
            target_id: target_id.into(),
            start,
            end,
            score: score.score,
            coverage: score.coverage,
            evaluated_seconds: score.target_seconds,
            trace,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(time: f64, midi: f32) -> PitchFrame {
        PitchFrame {
            time,
            midi,
            frequency: 440.0 * 2.0f32.powf((midi - 69.0) / 12.0),
            confidence: 1.0,
            rms: 0.1,
        }
    }
    fn target() -> Vec<PitchFrame> {
        (0..100).map(|i| frame(i as f64 * 0.02, 69.0)).collect()
    }
    #[test]
    fn exact_pitch_and_octave_are_distinct() {
        let f = target();
        let mut s = PracticeScorer::new();
        s.begin_segment(0.0);
        s.sample(&f, 0.0, frame(0.0, 69.0), 50.0);
        assert_eq!(s.snapshot().score, 100.0);
        s.sample(&f, 0.02, frame(0.02, 81.0), 50.0);
        assert_eq!(s.snapshot().score, 50.0);
        assert_eq!(s.snapshot().coverage, 100.0);
    }
    #[test]
    fn target_silence_is_not_an_error() {
        let mut f = target();
        for p in &mut f[10..20] {
            p.frequency = 0.0;
            p.confidence = 0.0;
        }
        let mut s = PracticeScorer::new();
        s.begin_segment(0.0);
        s.advance(&f, 0.38);
        assert!((s.snapshot().target_seconds - 0.2).abs() < 0.001);
        assert_eq!(s.snapshot().coverage, 0.0);
    }
    #[test]
    fn segment_resume_does_not_score_a_gap() {
        let f = target();
        let mut s = PracticeScorer::new();
        s.begin_segment(0.0);
        s.advance(&f, 0.02);
        s.begin_segment(1.0);
        s.advance(&f, 1.02);
        assert!((s.snapshot().target_seconds - 0.08).abs() < 0.001);
    }
    #[test]
    fn replacement_and_sparse_captures_do_not_double_count() {
        let f = target();
        let mut s = PracticeScorer::new();
        s.begin_segment(0.0);
        s.advance(&f, 0.04);
        s.sample(&f, 0.0, frame(0.0, 69.0), 50.0);
        s.sample(&f, 0.04, frame(0.04, 69.0), 50.0);
        s.sample(&f, 0.04, frame(0.04, 69.0), 50.0);
        assert_eq!(s.snapshot().score, 100.0);
        assert!((s.snapshot().target_seconds - 0.06).abs() < 0.001);
    }
    #[test]
    fn delayed_denominator_preserves_capture_tail() {
        assert!(capture_evaluation_delay(4096, 48000, 100.0) > 0.22);
        assert_eq!(evaluated_position(1.0, 0.9, 0.5), 0.9);
    }
    #[test]
    fn persisted_take_contains_a_snapshot() {
        let f = target();
        let mut s = PracticeScorer::new();
        s.begin_segment(0.0);
        s.sample(&f, 0.0, frame(0.0, 69.0), 50.0);
        let take = s.to_take("target", 0.0, 1.0, vec![frame(0.0, 69.0)]);
        assert_eq!(take.score, 100.0);
        assert_eq!(take.target_id, "target");
        assert_eq!(take.trace.len(), 1);
    }
}
