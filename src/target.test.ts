import { describe, it, expect } from "vitest";
import { canScoreTarget, selectTargetLine } from "./target";
import { pitchAt, PracticeScorer } from "./practice";
import type { AudioTrack, NoteSegment } from "./types";
const chord: NoteSegment[] = [
  { start: 0, end: 1, midi: 60 },
  { start: 0, end: 1, midi: 64 },
  { start: 0, end: 1, midi: 67 },
];
describe("polyphonic target contract", () => {
  it("excludes candidates outside the microphone detector range even if requested", () => {
    expect(
      selectTargetLine([{ start: 0, end: 1, midi: 96 }], 1, {
        mode: "highest",
        minMidi: 36,
        maxMidi: 96,
      }).notes,
    ).toEqual([]);
  });
  it("never scores an unreviewed polyphonic target", () => {
    const track = { analysisMode: "polyphonic", notes: chord } as AudioTrack;
    expect(canScoreTarget(track, false)).toBe(false);
    expect(canScoreTarget(track, true)).toBe(true);
    expect(canScoreTarget({ ...track, notes: [] }, true)).toBe(false);
  });
  it("selects a register and envelope without mixing all notes into the scored target", () => {
    const high = selectTargetLine(chord, 1, {
      mode: "highest",
      minMidi: 36,
      maxMidi: 96,
    });
    const low = selectTargetLine(chord, 1, {
      mode: "lowest",
      minMidi: 36,
      maxMidi: 96,
    });
    const middle = selectTargetLine(chord, 1, {
      mode: "highest",
      minMidi: 62,
      maxMidi: 65,
    });
    expect(pitchAt(high.pitch, 0.5)?.midi).toBe(67);
    expect(pitchAt(low.pitch, 0.5)?.midi).toBe(60);
    expect(pitchAt(middle.pitch, 0.5)?.midi).toBe(64);
    const score = new PracticeScorer().sample(
      high.pitch,
      0.5,
      { time: 0.5, midi: 60, frequency: 261.63, confidence: 1, rms: 0.1 },
      50,
    );
    expect(score.score).toBe(0);
  });
  it("keeps gaps silent and missing-register selections empty", () => {
    const result = selectTargetLine(
      [
        { start: 0.2, end: 0.4, midi: 60 },
        { start: 0.8, end: 1, midi: 64 },
      ],
      1,
      { mode: "highest", minMidi: 36, maxMidi: 96 },
    );
    expect(pitchAt(result.pitch, 0.6)).toBeNull();
    expect(
      selectTargetLine(chord, 1, { mode: "lowest", minMidi: 80, maxMidi: 90 })
        .notes,
    ).toEqual([]);
  });
  it("makes crossings explicit as an envelope rather than pretending to identify a singer", () => {
    const notes = [
      { start: 0, end: 0.5, midi: 60 },
      { start: 0, end: 1, midi: 64 },
      { start: 0.5, end: 1, midi: 67 },
    ];
    const result = selectTargetLine(notes, 1, {
      mode: "highest",
      minMidi: 36,
      maxMidi: 96,
    });
    expect(pitchAt(result.pitch, 0.25)?.midi).toBe(64);
    expect(pitchAt(result.pitch, 0.75)?.midi).toBe(67);
  });
});
