import type { PitchFrame, PracticeStats } from "./types";
export const noteName = (midi: number | null | undefined): string => {
  if (midi == null || !Number.isFinite(midi)) return "—";
  const rounded = Math.round(midi);
  return (
    ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"][
      ((rounded % 12) + 12) % 12
    ] +
    (Math.floor(rounded / 12) - 1)
  );
};
export function formatTime(seconds: number, decimals = false): string {
  const safe = Math.max(0, seconds || 0);
  return `${Math.floor(safe / 60)
    .toString()
    .padStart(2, "0")}:${Math.floor(safe % 60)
    .toString()
    .padStart(2, "0")}${decimals ? "." + Math.floor((safe % 1) * 10) : ""}`;
}
export function pitchAt(frames: PitchFrame[], time: number): PitchFrame | null {
  let low = 0,
    high = frames.length - 1;
  while (low <= high) {
    const mid = (low + high) >>> 1;
    if (frames[mid].time < time) low = mid + 1;
    else high = mid - 1;
  }
  const a = frames[low],
    b = frames[low - 1];
  const frame = !a
    ? b
    : !b
      ? a
      : Math.abs(a.time - time) < Math.abs(b.time - time)
        ? a
        : b;
  return frame &&
    Math.abs(frame.time - time) <= 0.065 &&
    frame.frequency > 0 &&
    frame.confidence >= 0.7 &&
    Number.isFinite(frame.midi)
    ? frame
    : null;
}
export class PracticeScorer {
  private bins = new Map<number, number>();
  private hits = 0;
  private sung = 0;
  private cursor: number | null = null;
  private cents: number | null = null;
  private targetMidi: number | null = null;
  private previousSample: number | null = null;
  reset() {
    this.bins.clear();
    this.hits = 0;
    this.sung = 0;
    this.cursor = null;
    this.cents = null;
    this.targetMidi = null;
    this.previousSample = null;
  }
  private write(bin: number, value: number) {
    const old = this.bins.get(bin) ?? 0;
    this.hits += (value === 3 ? 1 : 0) - (old === 3 ? 1 : 0);
    this.sung += (value > 0 ? 1 : 0) - (old > 0 ? 1 : 0);
    this.bins.set(bin, value);
  }
  private snapshot(): PracticeStats {
    const n = this.bins.size;
    return {
      score: n ? (100 * this.hits) / n : 0,
      coverage: n ? (100 * this.sung) / n : 0,
      cents: this.cents,
      targetMidi: this.targetMidi,
      matchedSeconds: this.hits * 0.02,
      targetSeconds: n * 0.02,
    };
  }
  /** The transport supplies the denominator, including missing microphone frames. */
  advance(frames: PitchFrame[], time: number): PracticeStats {
    if (this.cursor !== null && time >= this.cursor) {
      for (
        let bin = Math.floor(this.cursor / 0.02);
        bin <= Math.floor(time / 0.02);
        bin++
      ) {
        if (!this.bins.has(bin) && pitchAt(frames, bin * 0.02))
          this.write(bin, 0);
      }
    }
    this.cursor = time;
    return this.snapshot();
  }
  sample(
    frames: PitchFrame[],
    time: number,
    voice: PitchFrame,
    tolerance: number,
  ): PracticeStats {
    const target = pitchAt(frames, time);
    const voiced =
      voice.confidence >= 0.7 &&
      voice.frequency > 0 &&
      Number.isFinite(voice.midi);
    this.cents = target && voiced ? (voice.midi - target.midi) * 100 : null;
    this.targetMidi = target?.midi ?? null;
    if (time >= 0) {
      // A capture frame describes an interval, not a single 20 ms point.
      // Fill adjacent bins to avoid systematic misses at 44.1/48 kHz hop rates.
      const endBin = Math.floor(time / 0.02);
      const startBin =
        this.previousSample === null
          ? endBin
          : Math.max(
              Math.min(endBin, Math.floor(this.previousSample / 0.02) + 1),
              Math.ceil((time - 0.06) / 0.02),
            );
      for (let bin = startBin; bin <= endBin; bin++) {
        const reference = pitchAt(frames, bin * 0.02);
        if (reference) {
          const hit =
            voiced &&
            Math.abs((voice.midi - reference.midi) * 100) <= tolerance;
          this.write(bin, hit ? 3 : voiced ? 1 : 0);
        }
      }
      this.previousSample = time;
    }
    return this.snapshot();
  }
}
export const emptyStats: PracticeStats = {
  score: 0,
  coverage: 0,
  cents: null,
  targetMidi: null,
  matchedSeconds: 0,
  targetSeconds: 0,
};
export function intervalName(semitones: number): string {
  const n = Math.abs(semitones);
  const label =
    [
      "uníssono",
      "2ª menor",
      "2ª maior",
      "3ª menor",
      "3ª maior",
      "4ª justa",
      "trítono",
      "5ª justa",
      "6ª menor",
      "6ª maior",
      "7ª menor",
      "7ª maior",
      "oitava",
    ][n] ?? `${n} semitons`;
  return `${semitones > 0 ? "↑ " : semitones < 0 ? "↓ " : ""}${label}`;
}
