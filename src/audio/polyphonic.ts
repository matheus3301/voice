import type { NoteSegment } from "../types";

export interface AnalysisChunk {
  start: number;
  end: number;
  coreStart: number;
  coreEnd: number;
}
export interface ModelNote {
  startTimeSeconds: number;
  durationSeconds: number;
  pitchMidi: number;
  amplitude: number;
}
export interface BoundaryNote extends NoteSegment {
  crossesLeft: boolean;
  crossesRight: boolean;
}

export function planAnalysisChunks(
  duration: number,
  coreSeconds = 20,
  contextSeconds = 0.5,
): AnalysisChunk[] {
  if (
    !Number.isFinite(duration) ||
    duration <= 0 ||
    coreSeconds <= 0 ||
    contextSeconds < 0
  )
    return [];
  const chunks: AnalysisChunk[] = [];
  for (let coreStart = 0; coreStart < duration; coreStart += coreSeconds) {
    const coreEnd = Math.min(duration, coreStart + coreSeconds);
    chunks.push({
      start: Math.max(0, coreStart - contextSeconds),
      end: Math.min(duration, coreEnd + contextSeconds),
      coreStart,
      coreEnd,
    });
  }
  return chunks;
}

/** Only retain each chunk's core. Context supplies information, never duplicate output. */
export function trimChunkNotes(
  notes: ModelNote[],
  chunk: AnalysisChunk,
): BoundaryNote[] {
  const result: BoundaryNote[] = [];
  for (const note of notes) {
    const start = chunk.start + note.startTimeSeconds;
    const end = start + note.durationSeconds;
    if (
      ![start, end, note.pitchMidi, note.amplitude].every(Number.isFinite) ||
      end <= start ||
      note.pitchMidi < 21 ||
      note.pitchMidi > 108
    )
      continue;
    const clippedStart = Math.max(chunk.coreStart, start);
    const clippedEnd = Math.min(chunk.coreEnd, end);
    if (clippedEnd - clippedStart < 0.025) continue;
    result.push({
      start: clippedStart,
      end: clippedEnd,
      midi: Math.round(note.pitchMidi),
      crossesLeft: start < chunk.coreStart - 0.025,
      crossesRight: end > chunk.coreEnd + 0.025,
    });
  }
  return result;
}

/** Join only notes independently predicted to cross the same chunk boundary. */
export function mergeBoundaryNotes(notes: BoundaryNote[]): NoteSegment[] {
  const sorted = [...notes].sort(
    (a, b) => a.midi - b.midi || a.start - b.start,
  );
  const merged: BoundaryNote[] = [];
  for (const note of sorted) {
    const previous = merged.at(-1);
    if (
      previous &&
      previous.midi === note.midi &&
      previous.crossesRight &&
      note.crossesLeft &&
      Math.abs(previous.end - note.start) < 0.002
    ) {
      previous.end = note.end;
      previous.crossesRight = note.crossesRight;
    } else merged.push({ ...note });
  }
  return merged
    .map(({ start, end, midi }) => ({ start, end, midi }))
    .sort((a, b) => a.start - b.start || a.midi - b.midi);
}
