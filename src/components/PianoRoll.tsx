import { useEffect, useMemo, useRef, useState } from "react";
import type {
  PointerEvent as ReactPointerEvent,
  KeyboardEvent as ReactKeyboardEvent,
} from "react";
import type { AudioTrack, LiveSample, NoteSegment } from "../types";

interface Props {
  track: AudioTrack | null;
  time: number;
  live: LiveSample[];
  windowSeconds: number;
  onSeek: (seconds: number) => void;
  isPlaying: boolean;
  candidateNotes?: NoteSegment[];
}

const KEY_WIDTH = 59;
const RULER_HEIGHT = 32;
const NOTE_NAMES = [
  "C",
  "C♯",
  "D",
  "D♯",
  "E",
  "F",
  "F♯",
  "G",
  "G♯",
  "A",
  "A♯",
  "B",
];
const isBlack = (midi: number) =>
  [1, 3, 6, 8, 10].includes(((midi % 12) + 12) % 12);
const noteName = (midi: number) =>
  `${NOTE_NAMES[((midi % 12) + 12) % 12]}${Math.floor(midi / 12) - 1}`;
const clock = (seconds: number) =>
  `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;

function firstAtOrAfter<T extends { time: number }>(
  frames: T[],
  time: number,
): number {
  let low = 0;
  let high = frames.length;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (frames[mid].time < time) low = mid + 1;
    else high = mid;
  }
  return Math.max(0, low - 1);
}

function roundedRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
) {
  ctx.beginPath();
  ctx.roundRect(x, y, width, height, Math.min(radius, width / 2, height / 2));
}

export default function PianoRoll({
  track,
  time,
  live,
  windowSeconds,
  onSeek,
  isPlaying,
  candidateNotes,
}: Props) {
  const wrapper = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const drag = useRef<{ start: number; window: number } | null>(null);
  const [size, setSize] = useState({ width: 960, height: 428 });
  const visibleSeconds = Math.max(2, windowSeconds);
  const viewStart = Math.max(0, time - visibleSeconds * 0.28);
  const duration = track?.buffer.duration ?? 0;
  const pitchRange = useMemo(() => {
    const pitches = [...(track?.notes ?? []), ...(candidateNotes ?? [])]
      .map((note) => note.midi)
      .filter(Number.isFinite);
    let low = pitches.length
      ? Math.floor(
          pitches.reduce((min, midi) => Math.min(min, midi), Infinity),
        ) - 4
      : 45;
    let high = pitches.length
      ? Math.ceil(
          pitches.reduce((max, midi) => Math.max(max, midi), -Infinity),
        ) + 4
      : 68;
    if (high - low < 17) {
      const middle = Math.round((low + high) / 2);
      low = middle - 9;
      high = middle + 9;
    }
    // Keep an isolated transcription outlier from collapsing the useful range.
    if (!candidateNotes?.length && high - low > 48) {
      const sorted = [...pitches].sort((a, b) => a - b);
      const middle = sorted[Math.floor(sorted.length / 2)] ?? 57;
      low = Math.floor(middle) - 24;
      high = Math.floor(middle) + 24;
    }
    return { low, high };
  }, [track, candidateNotes]);

  useEffect(() => {
    const node = wrapper.current;
    if (!node) return;
    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      setSize({ width: Math.max(1, width), height: Math.max(200, height) });
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const node = canvas.current;
    if (!node) return;
    const ctx = node.getContext("2d");
    if (!ctx) return;
    const { width, height } = size;
    const pixelRatio = Math.min(window.devicePixelRatio || 1, 2);
    if (
      node.width !== Math.round(width * pixelRatio) ||
      node.height !== Math.round(height * pixelRatio)
    ) {
      node.width = Math.round(width * pixelRatio);
      node.height = Math.round(height * pixelRatio);
    }
    ctx.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = "#121717";
    ctx.fillRect(0, 0, width, height);

    const { low, high } = pitchRange;
    const rows = high - low + 1;
    const rowHeight = (height - RULER_HEIGHT) / rows;
    const plotWidth = Math.max(1, width - KEY_WIDTH);
    const x = (at: number) =>
      KEY_WIDTH + ((at - viewStart) / visibleSeconds) * plotWidth;
    const y = (midi: number) => RULER_HEIGHT + (high - midi + 0.5) * rowHeight;

    ctx.font = "10px ui-monospace, SFMono-Regular, Menlo, monospace";
    ctx.textBaseline = "middle";
    for (let midi = low; midi <= high; midi++) {
      const top = y(midi) - rowHeight / 2;
      const black = isBlack(midi);
      ctx.fillStyle = black ? "#141919" : "#181e1d";
      ctx.fillRect(KEY_WIDTH, top, plotWidth, rowHeight);
      ctx.strokeStyle = midi % 12 === 0 ? "#34403b" : "#222b28";
      ctx.beginPath();
      ctx.moveTo(KEY_WIDTH, top + rowHeight);
      ctx.lineTo(width, top + rowHeight);
      ctx.stroke();
      ctx.fillStyle = black ? "#111515" : "#26302c";
      ctx.fillRect(0, top, KEY_WIDTH - 1, rowHeight - 1);
      if (black) {
        ctx.fillStyle = "#1b2220";
        ctx.fillRect(0, top + 1, KEY_WIDTH * 0.65, rowHeight - 3);
      }
      if (rowHeight > 10) {
        ctx.textAlign = "right";
        ctx.fillStyle = midi % 12 === 0 ? "#c5d2cb" : "#7e8b84";
        ctx.fillText(noteName(midi), KEY_WIDTH - 10, y(midi));
      }
    }

    const tick =
      visibleSeconds <= 6
        ? 0.5
        : visibleSeconds <= 14
          ? 1
          : visibleSeconds <= 30
            ? 2
            : 5;
    ctx.fillStyle = "#1b2220";
    ctx.fillRect(KEY_WIDTH, 0, plotWidth, RULER_HEIGHT);
    for (
      let at = Math.ceil(viewStart / tick) * tick;
      at <= viewStart + visibleSeconds;
      at += tick
    ) {
      const pixel = x(at);
      ctx.strokeStyle = "#29332e";
      ctx.beginPath();
      ctx.moveTo(pixel, RULER_HEIGHT);
      ctx.lineTo(pixel, height);
      ctx.stroke();
      ctx.strokeStyle = "#4a5950";
      ctx.beginPath();
      ctx.moveTo(pixel, RULER_HEIGHT - 6);
      ctx.lineTo(pixel, RULER_HEIGHT);
      ctx.stroke();
      ctx.fillStyle = "#8a988f";
      ctx.textAlign = "left";
      ctx.fillText(at % 1 ? `${clock(at)}.5` : clock(at), pixel + 7, 15);
    }
    ctx.fillStyle = "#1b2220";
    ctx.fillRect(0, 0, KEY_WIDTH, RULER_HEIGHT);
    ctx.fillStyle = "#65746c";
    ctx.textAlign = "center";
    ctx.font = "9px ui-sans-serif, system-ui, sans-serif";
    ctx.fillText("NOTA", KEY_WIDTH / 2, 16);

    ctx.save();
    ctx.beginPath();
    ctx.rect(KEY_WIDTH, RULER_HEIGHT, plotWidth, height - RULER_HEIGHT);
    ctx.clip();
    // Keep the full transcription visible behind the explicitly chosen line.
    for (const note of candidateNotes ?? []) {
      if (note.end < viewStart || note.start > viewStart + visibleSeconds)
        continue;
      const left = x(note.start);
      const noteHeight = Math.min(15, rowHeight * 0.6);
      roundedRect(
        ctx,
        left,
        y(note.midi) - noteHeight / 2,
        Math.max(2, x(note.end) - left - 1),
        noteHeight,
        2,
      );
      ctx.fillStyle = "#a9b7ad18";
      ctx.fill();
      ctx.strokeStyle = "#a9b7ad35";
      ctx.lineWidth = 1;
      ctx.stroke();
    }
    for (const note of track?.notes ?? []) {
      if (note.end < viewStart) continue;
      if (note.start > viewStart + visibleSeconds) break;
      const left = x(note.start);
      const noteWidth = Math.max(2, x(note.end) - left - 1);
      const noteHeight = Math.min(17, rowHeight * 0.66);
      roundedRect(
        ctx,
        left,
        y(note.midi) - noteHeight / 2,
        noteWidth,
        noteHeight,
        3,
      );
      ctx.fillStyle = "#a8e4ca25";
      ctx.fill();
      ctx.strokeStyle = "#a8e4ca88";
      ctx.lineWidth = 1;
      ctx.stroke();
      if (noteWidth > 42 && noteHeight > 11) {
        ctx.font = "10px ui-monospace, SFMono-Regular, Menlo, monospace";
        ctx.fillStyle = "#b8e5d0";
        ctx.textAlign = "left";
        ctx.fillText(
          noteName(Math.round(note.midi)),
          Math.max(KEY_WIDTH + 5, left + 7),
          y(note.midi),
        );
      }
    }

    const frames = track?.pitch ?? [];
    ctx.strokeStyle = "#a8e4ca88";
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    let connected = false;
    let previousTime = -1;
    for (
      let index = firstAtOrAfter(frames, viewStart);
      index < frames.length;
      index++
    ) {
      const frame = frames[index];
      if (frame.time > viewStart + visibleSeconds) break;
      if (
        !Number.isFinite(frame.midi) ||
        frame.midi <= 0 ||
        frame.confidence < 0.65
      ) {
        connected = false;
        continue;
      }
      const px = x(frame.time);
      const py = y(frame.midi);
      if (!connected || frame.time - previousTime > 0.12) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
      previousTime = frame.time;
      connected = true;
    }
    ctx.stroke();

    ctx.strokeStyle = "#efb56f";
    ctx.lineWidth = 2.6;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.beginPath();
    connected = false;
    previousTime = -1;
    let lastLive: LiveSample | undefined;
    for (
      let index = firstAtOrAfter(live, viewStart);
      index < live.length;
      index++
    ) {
      const frame = live[index];
      if (frame.time > viewStart + visibleSeconds) break;
      if (
        !Number.isFinite(frame.midi) ||
        frame.midi <= 0 ||
        frame.confidence < 0.65
      ) {
        connected = false;
        continue;
      }
      const px = x(frame.time);
      const py = y(frame.midi);
      if (!connected || frame.time - previousTime > 0.18) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
      previousTime = frame.time;
      connected = true;
      lastLive = frame;
    }
    ctx.stroke();
    if (lastLive && Math.abs(time - lastLive.time) < 0.35) {
      ctx.beginPath();
      ctx.arc(x(lastLive.time), y(lastLive.midi), 4, 0, Math.PI * 2);
      ctx.fillStyle = "#efb56f";
      ctx.fill();
      ctx.beginPath();
      ctx.arc(x(lastLive.time), y(lastLive.midi), 8, 0, Math.PI * 2);
      ctx.strokeStyle = "#efb56f55";
      ctx.lineWidth = 1;
      ctx.stroke();
    }
    ctx.restore();

    const cursorX = x(time);
    ctx.strokeStyle = isPlaying ? "#f4f6f0" : "#a3b0a6";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(cursorX + 0.5, 0);
    ctx.lineTo(cursorX + 0.5, height);
    ctx.stroke();
    ctx.fillStyle = isPlaying ? "#f4f6f0" : "#a3b0a6";
    ctx.beginPath();
    ctx.moveTo(cursorX - 5, 0);
    ctx.lineTo(cursorX + 5, 0);
    ctx.lineTo(cursorX + 5, 6);
    ctx.lineTo(cursorX, 11);
    ctx.lineTo(cursorX - 5, 6);
    ctx.closePath();
    ctx.fill();
    ctx.strokeStyle = "#344038";
    ctx.beginPath();
    ctx.moveTo(KEY_WIDTH - 0.5, 0);
    ctx.lineTo(KEY_WIDTH - 0.5, height);
    ctx.stroke();
  }, [
    track,
    time,
    live,
    visibleSeconds,
    viewStart,
    size,
    isPlaying,
    pitchRange,
    candidateNotes,
  ]);

  const seekPointer = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    if (!track) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const active = drag.current ?? { start: viewStart, window: visibleSeconds };
    const fraction =
      (event.clientX - bounds.left - KEY_WIDTH) /
      Math.max(1, bounds.width - KEY_WIDTH);
    onSeek(
      Math.max(0, Math.min(duration, active.start + fraction * active.window)),
    );
  };
  const onKeyDown = (event: ReactKeyboardEvent<HTMLCanvasElement>) => {
    if (
      !track ||
      !["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)
    )
      return;
    event.preventDefault();
    onSeek(
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? Math.max(0, duration - 0.01)
          : Math.max(
              0,
              Math.min(
                duration,
                time +
                  (event.key === "ArrowRight" ? 1 : -1) *
                    (event.shiftKey ? 5 : 1),
              ),
            ),
    );
  };

  return (
    <div className="piano-roll" ref={wrapper}>
      <canvas
        ref={canvas}
        role="slider"
        aria-label="Posição na melodia. Use as setas para avançar ou voltar."
        aria-valuemin={0}
        aria-valuemax={Math.round(duration)}
        aria-valuenow={Math.round(time)}
        aria-valuetext={clock(time)}
        tabIndex={track ? 0 : -1}
        onKeyDown={onKeyDown}
        onPointerDown={(event) => {
          if (!track) return;
          drag.current = { start: viewStart, window: visibleSeconds };
          event.currentTarget.setPointerCapture(event.pointerId);
          seekPointer(event);
        }}
        onPointerMove={(event) => {
          if (drag.current) seekPointer(event);
        }}
        onPointerUp={(event) => {
          drag.current = null;
          if (event.currentTarget.hasPointerCapture(event.pointerId))
            event.currentTarget.releasePointerCapture(event.pointerId);
        }}
        onPointerCancel={() => {
          drag.current = null;
        }}
      />
      {!track && (
        <div className="empty-roll">
          <span>Sem melodia carregada</span>
          <p>Importe uma voz alvo para visualizar as notas.</p>
        </div>
      )}
      {track && track.notes.length === 0 && !candidateNotes?.length && (
        <div className="empty-roll">
          <span>Nenhuma nota detectada</span>
          <p>Use uma voz isolada e confira o nível da gravação.</p>
        </div>
      )}
    </div>
  );
}
