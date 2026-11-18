import type { AudioTrack, NoteSegment, PitchFrame } from "./types";
import {
  MAX_FREQUENCY,
  MIN_FREQUENCY,
  midiToFrequency,
  segmentNotes,
} from "./audio/pitch";

export interface TargetSelection {
  mode: "highest" | "lowest";
  minMidi: number;
  maxMidi: number;
}
export const defaultTargetSelection: TargetSelection = {
  mode: "highest",
  minMidi: 36,
  maxMidi: 86,
};
export function isPolyphonic(track: AudioTrack | null): boolean {
  return track?.analysisMode === "polyphonic";
}
export function canScoreTarget(
  track: AudioTrack | null,
  reviewed: boolean,
): boolean {
  return (
    !!track && (!isPolyphonic(track) || reviewed) && track.notes.length > 0
  );
}
/** A register/envelope preview, explicitly not singer or choir-part identification. */
export function selectTargetLine(
  notes: NoteSegment[],
  duration: number,
  selection: TargetSelection,
): { pitch: PitchFrame[]; notes: NoteSegment[] } {
  const candidates = notes
    .filter(
      (n) =>
        Number.isFinite(n.midi) &&
        n.midi >= selection.minMidi &&
        n.midi <= selection.maxMidi &&
        midiToFrequency(n.midi) >= MIN_FREQUENCY &&
        midiToFrequency(n.midi) <= MAX_FREQUENCY &&
        n.end > n.start,
    )
    .sort((a, b) => a.start - b.start);
  const active = new Set<NoteSegment>();
  const pitch: PitchFrame[] = [];
  let next = 0;
  for (let index = 0; index <= Math.ceil(duration / 0.025); index++) {
    const time = index * 0.025;
    while (next < candidates.length && candidates[next].start <= time)
      active.add(candidates[next++]);
    let chosen: NoteSegment | null = null;
    for (const candidate of active) {
      if (candidate.end <= time) {
        active.delete(candidate);
        continue;
      }
      if (
        !chosen ||
        (selection.mode === "highest"
          ? candidate.midi > chosen.midi
          : candidate.midi < chosen.midi)
      )
        chosen = candidate;
    }
    // Confidence marks a usable symbolic target, not model confidence.
    pitch.push({
      time,
      midi: chosen?.midi ?? NaN,
      frequency: chosen ? midiToFrequency(chosen.midi) : 0,
      confidence: chosen ? 1 : 0,
      rms: chosen ? 0.1 : 0,
    });
  }
  return { pitch, notes: segmentNotes(pitch, duration) };
}
