import type { NoteSegment, PitchFrame } from "../types";

export const ANALYSIS_RATE = 16_000;
export const ANALYSIS_HOP = 0.025;
export const MIN_FREQUENCY = 65;
export const MAX_FREQUENCY = 1_200;

export function frequencyToMidi(frequency: number): number {
  return frequency > 0 ? 69 + 12 * Math.log2(frequency / 440) : Number.NaN;
}

export function midiToFrequency(midi: number): number {
  return 440 * 2 ** ((midi - 69) / 12);
}

/** YIN difference detector. Unvoiced frames retain RMS but never invent a note. */
export function detectPitch(
  samples: Float32Array,
  sampleRate: number,
  time = 0,
): PitchFrame {
  let mean = 0;
  for (let i = 0; i < samples.length; i++) mean += samples[i];
  mean /= samples.length || 1;
  let energy = 0;
  for (let i = 0; i < samples.length; i++) energy += (samples[i] - mean) ** 2;
  const rms = Math.sqrt(energy / (samples.length || 1));
  const empty = { time, frequency: 0, midi: Number.NaN, confidence: 0, rms };
  if (samples.length < 128 || rms < 0.006) return empty;

  const half = Math.floor(samples.length / 2);
  const minLag = Math.max(2, Math.floor(sampleRate / MAX_FREQUENCY));
  const maxLag = Math.min(half - 1, Math.ceil(sampleRate / MIN_FREQUENCY));
  if (maxLag <= minLag) return empty;
  const difference = new Float32Array(maxLag + 1);
  for (let lag = 1; lag <= maxLag; lag++) {
    let sum = 0;
    for (let j = 0; j < half; j++) {
      const delta = samples[j] - samples[j + lag];
      sum += delta * delta;
    }
    difference[lag] = sum;
  }
  let running = 0;
  difference[0] = 1;
  for (let lag = 1; lag <= maxLag; lag++) {
    running += difference[lag];
    difference[lag] = running > 0 ? (difference[lag] * lag) / running : 1;
  }
  let lag = minLag;
  for (; lag < maxLag; lag++) {
    if (difference[lag] < 0.15) {
      while (lag + 1 <= maxLag && difference[lag + 1] < difference[lag]) lag++;
      break;
    }
  }
  if (lag >= maxLag) {
    lag = minLag;
    for (let candidate = minLag + 1; candidate <= maxLag; candidate++) {
      if (difference[candidate] < difference[lag]) lag = candidate;
    }
    if (difference[lag] > 0.15) return empty;
  }
  const confidence = Math.max(0, 1 - difference[lag]);
  if (confidence < 0.82) return empty;
  let refinedLag = lag;
  if (lag > 1 && lag < maxLag) {
    const left = difference[lag - 1];
    const center = difference[lag];
    const right = difference[lag + 1];
    const denominator = left - 2 * center + right;
    if (Math.abs(denominator) > 1e-9)
      refinedLag += (left - right) / (2 * denominator);
  }
  const frequency = sampleRate / refinedLag;
  if (frequency < MIN_FREQUENCY || frequency > MAX_FREQUENCY) return empty;
  return { time, frequency, midi: frequencyToMidi(frequency), confidence, rms };
}

/** Box-filtered resampling reduces high-frequency aliasing before pitch analysis. */
export function resampleMono(
  channels: Float32Array[],
  sourceRate: number,
  targetRate = ANALYSIS_RATE,
): Float32Array {
  const sourceLength = channels[0]?.length ?? 0;
  const ratio = sourceRate / targetRate;
  const output = new Float32Array(Math.floor(sourceLength / ratio));
  if (!channels.length) return output;
  for (let i = 0; i < output.length; i++) {
    const start = i * ratio;
    const end = Math.min(sourceLength, (i + 1) * ratio);
    const first = Math.floor(start);
    const last = Math.ceil(end);
    let sum = 0;
    for (let source = first; source < last; source++) {
      const weight = Math.max(
        0,
        Math.min(source + 1, end) - Math.max(source, start),
      );
      for (const channel of channels) sum += (channel[source] ?? 0) * weight;
    }
    output[i] = sum / ((end - start) * channels.length || 1);
  }
  return output;
}

export function createWaveform(
  channels: Float32Array[],
  bins = 1_600,
): Float32Array {
  const length = channels[0]?.length ?? 0;
  const waveform = new Float32Array(Math.min(bins, length));
  for (let bin = 0; bin < waveform.length; bin++) {
    const start = Math.floor((bin * length) / waveform.length);
    const end = Math.max(
      start + 1,
      Math.floor(((bin + 1) * length) / waveform.length),
    );
    let peak = 0;
    for (const channel of channels) {
      for (let i = start; i < end; i++)
        peak = Math.max(peak, Math.abs(channel[i]));
    }
    waveform[bin] = peak;
  }
  return waveform;
}

export function segmentNotes(
  frames: PitchFrame[],
  duration: number,
  hop = ANALYSIS_HOP,
): NoteSegment[] {
  const notes: NoteSegment[] = [];
  let active: NoteSegment | null = null;
  for (let i = 0; i < frames.length; i++) {
    const frame = frames[i];
    const midi =
      Number.isFinite(frame.midi) && frame.confidence >= 0.82
        ? Math.round(frame.midi)
        : null;
    if (midi !== null) {
      const start = Math.max(0, frame.time - hop / 2);
      const end = Math.min(duration, frame.time + hop / 2);
      if (active && active.midi === midi && start - active.end < hop * 1.6)
        active.end = end;
      else {
        if (active && active.end - active.start >= 0.07) notes.push(active);
        active = { start, end, midi };
      }
    } else if (active) {
      if (active.end - active.start >= 0.07) notes.push(active);
      active = null;
    }
  }
  if (active && active.end - active.start >= 0.07) notes.push(active);
  return notes;
}

export function analyzePitch(
  samples: Float32Array,
  sampleRate: number,
  onProgress?: (value: number) => void,
): PitchFrame[] {
  const windowSize = 1_024;
  const hop = Math.round(sampleRate * ANALYSIS_HOP);
  const frames: PitchFrame[] = [];
  const window = new Float32Array(windowSize);
  for (let center = 0; center < samples.length; center += hop) {
    window.fill(0);
    const start = center - windowSize / 2;
    const from = Math.max(0, start);
    const to = Math.min(samples.length, start + windowSize);
    window.set(samples.subarray(from, to), from - start);
    frames.push(detectPitch(window, sampleRate, center / sampleRate));
    if (frames.length % 80 === 0) onProgress?.(center / samples.length);
  }
  // Remove isolated detector excursions without pulling voiced notes across silence.
  const original = frames.map((frame) => frame.midi);
  for (let i = 1; i < frames.length - 1; i++) {
    if (
      [original[i - 1], original[i], original[i + 1]].every(Number.isFinite)
    ) {
      const midi = [original[i - 1], original[i], original[i + 1]].sort(
        (a, b) => a - b,
      )[1];
      frames[i] = { ...frames[i], midi, frequency: midiToFrequency(midi) };
    }
  }
  onProgress?.(1);
  return frames;
}
