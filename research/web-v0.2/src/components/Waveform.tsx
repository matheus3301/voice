import { useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import type { AudioTrack, LoopRegion } from "../types";

interface Props {
  track: AudioTrack | null;
  time: number;
  duration: number;
  onSeek: (seconds: number) => void;
  color: string;
  loop?: LoopRegion;
}

export default function Waveform({
  track,
  time,
  duration,
  onSeek,
  color,
  loop,
}: Props) {
  const ref = useRef<HTMLCanvasElement>(null);
  const dragging = useRef(false);
  const [width, setWidth] = useState(640);
  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    const observer = new ResizeObserver(([entry]) =>
      setWidth(Math.max(1, entry.contentRect.width)),
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const canvas = ref.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;
    const height = 44;
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    if (
      canvas.width !== Math.round(width * ratio) ||
      canvas.height !== Math.round(height * ratio)
    ) {
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(height * ratio);
    }
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const trackWidth =
      duration > 0 && track
        ? (width * track.buffer.duration) / duration
        : width;
    const peaks = track?.waveform;
    if (loop?.enabled && duration > 0) {
      const left = (loop.start / duration) * width;
      const right = (loop.end / duration) * width;
      ctx.fillStyle = "#a8e4ca13";
      ctx.fillRect(left, 0, right - left, height);
      ctx.strokeStyle = "#a8e4ca66";
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(left, 0);
      ctx.lineTo(left, height);
      ctx.moveTo(right, 0);
      ctx.lineTo(right, height);
      ctx.stroke();
    }
    ctx.fillStyle = color;
    ctx.globalAlpha = 0.7;
    if (peaks?.length) {
      const columns = Math.ceil(trackWidth / 3);
      for (let index = 0; index < columns; index++) {
        const start = Math.floor((index / columns) * peaks.length);
        const end = Math.max(
          start + 1,
          Math.floor(((index + 1) / columns) * peaks.length),
        );
        let peak = 0;
        for (let n = start; n < end; n++)
          peak = Math.max(peak, Math.abs(peaks[n]));
        const amplitude = Math.max(1, Math.min(1, peak) * 34);
        ctx.fillRect(index * 3, (height - amplitude) / 2, 1.5, amplitude);
      }
    } else {
      ctx.globalAlpha = 0.25;
      ctx.fillRect(0, height / 2, width, 1);
    }
    ctx.globalAlpha = 1;
    if (duration > 0) {
      const playhead = (time / duration) * width;
      ctx.fillStyle = "#11161566";
      ctx.fillRect(playhead, 0, width - playhead, height);
      ctx.strokeStyle = "#dce6dc";
      ctx.beginPath();
      ctx.moveTo(playhead + 0.5, 0);
      ctx.lineTo(playhead + 0.5, height);
      ctx.stroke();
    }
  }, [track, time, duration, color, width, loop]);
  const seek = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    if (!track || duration <= 0) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    onSeek(
      Math.max(
        0,
        Math.min(
          duration,
          ((event.clientX - bounds.left) / bounds.width) * duration,
        ),
      ),
    );
  };
  return (
    <canvas
      className="waveform"
      ref={ref}
      role="slider"
      tabIndex={track ? 0 : -1}
      aria-label={`Posição em ${track?.name ?? "faixa vazia"}`}
      aria-valuemin={0}
      aria-valuemax={Math.round(duration)}
      aria-valuenow={Math.round(time)}
      onPointerDown={(event) => {
        if (!track) return;
        dragging.current = true;
        event.currentTarget.setPointerCapture(event.pointerId);
        seek(event);
      }}
      onPointerMove={(event) => {
        if (dragging.current) seek(event);
      }}
      onPointerUp={(event) => {
        dragging.current = false;
        if (event.currentTarget.hasPointerCapture(event.pointerId))
          event.currentTarget.releasePointerCapture(event.pointerId);
      }}
      onPointerCancel={() => {
        dragging.current = false;
      }}
      onKeyDown={(event) => {
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
                    time + (event.key === "ArrowRight" ? 1 : -1),
                  ),
                ),
        );
      }}
    />
  );
}
