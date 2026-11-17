import { useRef, useState } from "react";
import { AudioLines, Check, FileAudio, Upload } from "lucide-react";
import { formatTime } from "../practice";
import type { useStudio } from "../useStudio";
import type { TrackRole } from "../types";

type Studio = ReturnType<typeof useStudio>;

export default function UploadSlot({
  role,
  studio,
}: {
  role: TrackRole;
  studio: Studio;
}) {
  const fileInput = useRef<HTMLInputElement>(null);
  const [dragging, setDragging] = useState(false);
  const track = studio.tracks[role],
    progress = studio.progress[role];
  const target = role === "target";
  return (
    <section
      className={`upload-slot ${target ? "target-slot" : "backing-slot"} ${dragging ? "dragging" : ""} ${track ? "has-file" : ""}`}
      onDragOver={(e) => {
        e.preventDefault();
        setDragging(true);
      }}
      onDragLeave={() => setDragging(false)}
      onDrop={(e) => {
        e.preventDefault();
        setDragging(false);
        const f = e.dataTransfer.files[0];
        if (f && progress === null) void studio.loadFile(role, f);
      }}
    >
      <div className="upload-slot-top">
        <span className="track-number">{target ? "02" : "01"}</span>
        <span className="section-label">
          {target ? "VOZ ALVO" : "BACKING TRACK"}
        </span>
        {track && progress === null ? (
          <Check size={17} />
        ) : (
          <AudioLines size={19} />
        )}
      </div>
      <div className="upload-file-icon">
        <FileAudio size={30} strokeWidth={1.3} />
      </div>
      <h2>
        {track
          ? track.name
          : target
            ? "A melodia que você vai cantar"
            : "O acompanhamento"}
      </h2>
      <p>
        {track
          ? `${formatTime(track.buffer.duration)} · ${track.buffer.numberOfChannels} ${track.buffer.numberOfChannels === 1 ? "canal" : "canais"}`
          : target
            ? "Uma voz isolada ou uma linha de backing vocal."
            : "A base instrumental para tocar junto."}
      </p>
      <input
        ref={fileInput}
        type="file"
        accept="audio/*,.wav,.mp3,.m4a,.ogg,.flac,.aac"
        aria-label={target ? "Selecionar voz alvo" : "Selecionar backing track"}
        disabled={progress !== null}
        onChange={(e) => {
          const f = e.target.files?.[0];
          if (f) void studio.loadFile(role, f);
          e.target.value = "";
        }}
        className="file-picker"
      />
      <button
        className="button button-secondary"
        disabled={progress !== null}
        onClick={() => fileInput.current?.click()}
      >
        <Upload size={15} />
        {progress !== null
          ? "Analisando…"
          : track
            ? "Trocar arquivo"
            : "Selecionar arquivo"}
      </button>
      {progress !== null ? (
        <div className="upload-progress" role="status">
          <div className="progress-bar">
            <span style={{ width: `${Math.max(3, progress * 100)}%` }} />
          </div>
          <span>
            {target ? "Extraindo melodia" : "Preparando áudio"} ·{" "}
            {Math.round(progress * 100)}%
          </span>
        </div>
      ) : (
        <span className="drop-hint">
          {track
            ? "Arquivo carregado neste navegador"
            : "ou arraste o arquivo até aqui"}
        </span>
      )}
    </section>
  );
}
