import type { LoopRegion } from "../types";

export function validLoop(loop: LoopRegion, duration: number): LoopRegion {
  const start = Math.max(0, Math.min(duration, loop.start));
  const end = Math.max(start, Math.min(duration, loop.end));
  return { start, end, enabled: loop.enabled && end - start >= 0.1 };
}

/** Shared clock mapping for cursor and captured pitch, including the first partial loop. */
export function transportPosition(
  offset: number,
  elapsed: number,
  duration: number,
  loop: LoopRegion,
): number {
  const raw = Math.max(0, offset + elapsed);
  if (loop.enabled && raw >= loop.end)
    return loop.start + ((raw - loop.end) % (loop.end - loop.start));
  return Math.min(duration, raw);
}

export function loopIterationAt(
  offset: number,
  elapsed: number,
  loop: LoopRegion,
): number {
  const raw = offset + Math.max(0, elapsed);
  return loop.enabled && raw >= loop.end
    ? 1 + Math.floor((raw - loop.end) / (loop.end - loop.start))
    : 0;
}

/** Apply capture compensation before wrapping, and reject windows from an earlier transport run. */
export function capturePosition(
  audioTime: number,
  compensation: number,
  startedAt: number,
  playing: boolean,
  offset: number,
  duration: number,
  loop: LoopRegion,
): { time: number; iteration: number } | null {
  if (!playing) return { time: offset, iteration: 0 };
  const captureTime = audioTime - compensation;
  if (audioTime < startedAt || captureTime < startedAt) return null;
  const elapsed = captureTime - startedAt;
  return {
    time: transportPosition(offset, elapsed, duration, loop),
    iteration: loopIterationAt(offset, elapsed, loop),
  };
}
