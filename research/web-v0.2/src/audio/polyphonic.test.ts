import { describe, expect, it } from "vitest";
import {
  mergeBoundaryNotes,
  planAnalysisChunks,
  trimChunkNotes,
} from "./polyphonic";

describe("polyphonic candidate chunks", () => {
  it("covers the complete file once with overlap used only as context", () => {
    const chunks = planAnalysisChunks(45);
    expect(chunks).toEqual([
      { start: 0, end: 20.5, coreStart: 0, coreEnd: 20 },
      { start: 19.5, end: 40.5, coreStart: 20, coreEnd: 40 },
      { start: 39.5, end: 45, coreStart: 40, coreEnd: 45 },
    ]);
    expect(
      chunks.reduce((sum, chunk) => sum + chunk.coreEnd - chunk.coreStart, 0),
    ).toBe(45);
    expect(planAnalysisChunks(900)).toHaveLength(45);
  });
  it("retains simultaneous notes instead of collapsing polyphony to argmax", () => {
    const chunk = planAnalysisChunks(10)[0];
    const notes = trimChunkNotes(
      [
        {
          startTimeSeconds: 1,
          durationSeconds: 2,
          pitchMidi: 60,
          amplitude: 0.8,
        },
        {
          startTimeSeconds: 1,
          durationSeconds: 2,
          pitchMidi: 64,
          amplitude: 0.4,
        },
        {
          startTimeSeconds: 1,
          durationSeconds: 2,
          pitchMidi: 67,
          amplitude: 0.2,
        },
      ],
      chunk,
    );
    expect(mergeBoundaryNotes(notes)).toEqual([
      { start: 1, end: 3, midi: 60 },
      { start: 1, end: 3, midi: 64 },
      { start: 1, end: 3, midi: 67 },
    ]);
  });
  it("trims context and joins a sustained note across a chunk boundary", () => {
    const chunks = planAnalysisChunks(30);
    const left = trimChunkNotes(
      [
        {
          startTimeSeconds: 19,
          durationSeconds: 1.5,
          pitchMidi: 60,
          amplitude: 0.8,
        },
      ],
      chunks[0],
    );
    const right = trimChunkNotes(
      [
        {
          startTimeSeconds: 0,
          durationSeconds: 1.5,
          pitchMidi: 60,
          amplitude: 0.8,
        },
      ],
      chunks[1],
    );
    expect(mergeBoundaryNotes([...left, ...right])).toEqual([
      { start: 19, end: 21, midi: 60 },
    ]);
  });
  it("does not merge repeated attacks just because pitch matches at a boundary", () => {
    const chunks = planAnalysisChunks(30);
    const left = trimChunkNotes(
      [
        {
          startTimeSeconds: 19,
          durationSeconds: 1,
          pitchMidi: 60,
          amplitude: 0.8,
        },
      ],
      chunks[0],
    );
    const right = trimChunkNotes(
      [
        {
          startTimeSeconds: 0.5,
          durationSeconds: 1,
          pitchMidi: 60,
          amplitude: 0.8,
        },
      ],
      chunks[1],
    );
    expect(mergeBoundaryNotes([...left, ...right])).toEqual([
      { start: 19, end: 20, midi: 60 },
      { start: 20, end: 21, midi: 60 },
    ]);
  });
  it("ignores non-finite, impossible and context-only model events", () => {
    const chunk = planAnalysisChunks(30)[1];
    expect(
      trimChunkNotes(
        [
          {
            startTimeSeconds: 0,
            durationSeconds: 0.2,
            pitchMidi: 60,
            amplitude: 0.8,
          },
          {
            startTimeSeconds: NaN,
            durationSeconds: 1,
            pitchMidi: 60,
            amplitude: 0.8,
          },
          {
            startTimeSeconds: 1,
            durationSeconds: -2,
            pitchMidi: 60,
            amplitude: 0.8,
          },
          {
            startTimeSeconds: 1,
            durationSeconds: 2,
            pitchMidi: 200,
            amplitude: 0.8,
          },
        ],
        chunk,
      ),
    ).toEqual([]);
  });
});
