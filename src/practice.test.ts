import { describe, it, expect } from "vitest";
import { PracticeScorer, noteName, pitchAt, intervalName } from "./practice";
import type { PitchFrame } from "./types";
const frame = (time: number, midi = 69, frequency = 440): PitchFrame => ({
  time,
  midi,
  frequency,
  confidence: 1,
  rms: 0.2,
});
describe("musical feedback", () => {
  it("uses equal temperament, octave and directed interval labels", () => {
    expect(noteName(69)).toBe("A4");
    expect(noteName(60)).toBe("C4");
    expect(noteName(NaN)).toBe("—");
    expect(intervalName(-7)).toBe("↓ 5ª justa");
  });
  it("does not invent targets across silence or large gaps", () => {
    expect(pitchAt([frame(1)], 2)).toBeNull();
    expect(pitchAt([frame(1, NaN, 0)], 1)).toBeNull();
  });
  it("scores silence as a miss and excludes silence in the reference", () => {
    const s = new PracticeScorer(),
      target = [frame(0), frame(0.02)];
    s.sample(target, 0, frame(0), 50);
    const score = s.sample(target, 0.02, frame(0.02, NaN, 0), 50);
    expect(score.score).toBe(50);
    expect(score.coverage).toBe(50);
    expect(s.sample(target, 1, frame(1), 50).score).toBe(50);
  });
  it("counts target time when capture stops or drops frames", () => {
    const s = new PracticeScorer(),
      target = Array.from({ length: 11 }, (_, i) => frame(i * 0.02));
    s.advance(target, 0);
    s.sample(target, 0, frame(0), 50);
    const result = s.advance(target, 0.2);
    expect(result.coverage).toBeCloseTo(100 / 11);
    expect(result.score).toBeCloseTo(100 / 11);
    expect(result.targetSeconds).toBeCloseTo(0.22);
  });
  it("does not penalize a perfect continuous tone for the 48 kHz capture hop", () => {
    const s = new PracticeScorer(),
      target = Array.from({ length: 51 }, (_, i) => frame(i * 0.02));
    s.advance(target, 0);
    for (let t = 0; t < 1; t += 1024 / 48000) {
      s.advance(target, t);
      s.sample(target, t, frame(t), 50);
    }
    expect(s.advance(target, 0.98).score).toBe(100);
  });
  it("counts wrong octaves as wrong and returns signed cents", () => {
    const result = new PracticeScorer().sample(
      [frame(0)],
      0,
      frame(0, 81, 880),
      50,
    );
    expect(result.score).toBe(0);
    expect(result.cents).toBe(1200);
  });
  it("overwrites samples in a time bin, respects tolerance and resets", () => {
    const s = new PracticeScorer();
    s.sample([frame(0)], 0, frame(0, 69.8), 50);
    expect(s.sample([frame(0)], 0.001, frame(0, 69.2), 50).score).toBe(100);
    s.reset();
    expect(s.sample([frame(0)], 0, frame(0, 69.8), 50).score).toBe(0);
  });
});
