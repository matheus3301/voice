import { describe, expect, it } from "vitest";
import {
  analyzePitch,
  createWaveform,
  detectPitch,
  frequencyToMidi,
  midiToFrequency,
  resampleMono,
  segmentNotes,
} from "./pitch";
import { capturePosition, transportPosition, validLoop } from "./transport";

function tone(
  frequency: number,
  sampleRate = 16_000,
  length = 1_024,
  cents = 0,
): Float32Array {
  return Float32Array.from(
    { length },
    (_, i) =>
      0.2 *
      Math.sin(
        (2 * Math.PI * frequency * 2 ** (cents / 1200) * i) / sampleRate,
      ),
  );
}

describe("pitch detection", () => {
  it.each([65.41, 110, 220, 440, 880, 1046.5])(
    "detects %.2f Hz within 8 cents",
    (frequency) => {
      const pitch = detectPitch(tone(frequency), 16_000);
      expect(
        Math.abs(1200 * Math.log2(pitch.frequency / frequency)),
      ).toBeLessThan(8);
      expect(pitch.confidence).toBeGreaterThan(0.9);
    },
  );
  it("preserves cents instead of quantizing the detected pitch", () => {
    expect(
      (detectPitch(tone(440, 16000, 1024, 35), 16000).midi - 69) * 100,
    ).toBeCloseTo(35, -0.5);
  });
  it("rejects silence and quiet input without inventing a note", () => {
    const silence = detectPitch(new Float32Array(2048), 48000);
    expect(silence.frequency).toBe(0);
    expect(Number.isNaN(silence.midi)).toBe(true);
    const quiet = tone(220).map((value) => value * 0.001);
    expect(detectPitch(quiet, 16000).frequency).toBe(0);
  });
  it("ignores a constant DC offset", () => {
    const shifted = tone(220).map((value) => value + 0.3);
    expect(detectPitch(shifted, 16000).frequency).toBeCloseTo(220, 0);
    expect(detectPitch(new Float32Array(1024).fill(0.3), 16000).frequency).toBe(
      0,
    );
  });
  it("rejects broadband noise rather than displaying a confident note", () => {
    let seed = 42;
    const noise = Float32Array.from({ length: 2048 }, () => {
      seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
      return (seed / 2 ** 32 - 0.5) * 0.3;
    });
    expect(detectPitch(noise, 16000).frequency).toBe(0);
  });
  it("keeps fundamentals with voice-like harmonics", () => {
    const samples = Float32Array.from({ length: 1024 }, (_, i) => {
      const phase = (2 * Math.PI * 147 * i) / 16000;
      return (
        0.12 * Math.sin(phase) +
        0.2 * Math.sin(phase * 2) +
        0.06 * Math.sin(phase * 3)
      );
    });
    expect(detectPitch(samples, 16000).frequency).toBeCloseTo(147, 0);
  });
  it("retains timing through resampling and file analysis", () => {
    const samples = resampleMono([tone(220, 48000, 48000)], 48000);
    expect(samples.length).toBe(16000);
    const frames = analyzePitch(samples, 16000);
    expect(frames[20].time).toBe(0.5);
    expect(frames[20].midi).toBeCloseTo(57, 1);
    const notes = segmentNotes(frames, 1);
    expect(notes).toHaveLength(1);
    expect(notes[0].midi).toBe(57);
    expect(notes[0].start).toBeLessThan(0.05);
    expect(notes[0].end).toBeGreaterThan(0.95);
  });
  it("never merges notes over a silent gap", () => {
    const samples = new Float32Array(32000);
    samples.set(tone(220, 16000, 8000), 0);
    samples.set(tone(220, 16000, 8000), 16000);
    const notes = segmentNotes(analyzePitch(samples, 16000), 2);
    expect(notes).toHaveLength(2);
    expect(notes[0].end).toBeLessThan(0.55);
    expect(notes[1].start).toBeGreaterThan(0.95);
  });
  it("builds a bounded peak waveform and valid MIDI conversions", () => {
    expect([
      ...createWaveform([new Float32Array([0.1, -0.5, 0.3, -0.2])], 2),
    ]).toEqual([0.5, expect.closeTo(0.3)]);
    expect(midiToFrequency(69)).toBe(440);
    expect(frequencyToMidi(220)).toBe(57);
  });
});

describe("transport clock", () => {
  it("pauses at the actual end for unequal track lengths", () => {
    expect(
      transportPosition(4, 50, 24, { start: 0, end: 0, enabled: false }),
    ).toBe(24);
  });
  it("plays an intro then wraps precisely inside A–B", () => {
    const loop = { start: 4, end: 8, enabled: true };
    expect(transportPosition(0, 3, 24, loop)).toBe(3);
    expect(transportPosition(0, 8, 24, loop)).toBe(4);
    expect(transportPosition(5, 4.25, 24, loop)).toBe(5.25);
    expect(transportPosition(5, 12, 24, loop)).toBe(5);
  });
  it("clamps and disables invalid or too-short loops", () => {
    expect(validLoop({ start: -2, end: 100, enabled: true }, 24)).toEqual({
      start: 0,
      end: 24,
      enabled: true,
    });
    expect(validLoop({ start: 5, end: 5.01, enabled: true }, 24).enabled).toBe(
      false,
    );
    expect(validLoop({ start: 7, end: 2, enabled: true }, 24).enabled).toBe(
      false,
    );
  });
  it("applies capture latency before loop wrap and identifies the correct attempt", () => {
    const loop = { start: 4, end: 8, enabled: true };
    const beforeBoundary = capturePosition(13.05, 0.1, 10, true, 5, 24, loop);
    expect(beforeBoundary?.time).toBeCloseTo(7.95);
    expect(beforeBoundary?.iteration).toBe(0);
    const afterBoundary = capturePosition(13.15, 0.1, 10, true, 5, 24, loop);
    expect(afterBoundary?.time).toBeCloseTo(4.05);
    expect(afterBoundary?.iteration).toBe(1);
  });
  it("rejects old capture windows after a seek and allows a paused tuner", () => {
    const loop = { start: 4, end: 8, enabled: false };
    expect(capturePosition(9.9, 0, 10, true, 5, 24, loop)).toBeNull();
    expect(capturePosition(9.9, -0.5, 10, true, 5, 24, loop)).toBeNull();
    expect(capturePosition(10.05, 0.1, 10, true, 5, 24, loop)).toBeNull();
    expect(capturePosition(5, 0.1, 10, false, 7, 24, loop)).toEqual({
      time: 7,
      iteration: 0,
    });
  });
});
