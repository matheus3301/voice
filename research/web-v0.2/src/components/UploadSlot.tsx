import { useRef, useState } from "react";
import { AudioLines, Check, FileAudio, Upload, X } from "lucide-react";
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
  const busy = Object.values(studio.progress).some((value) => value !== null);
  // A singer selects the kind of reference before importing. Keep the existing
  // mint-on-graphite hierarchy, inset native controls and compact 4px rhythm.
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
        if (f && !busy) void studio.loadFile(role, f);
      }}
    >
      <div className="upload-slot-top">
        <span className="track-number">{target ? "01" : "02"}</span>
        <span className="section-label">
          {target ? "FAIXA ALVO · OBRIGATÓRIA" : "BACKING TRACK · OPCIONAL"}
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
            ? "A faixa que você quer cantar"
            : "Um acompanhamento, se quiser"}
      </h2>
      <p>
        {track
          ? `${formatTime(track.buffer.duration)} · ${track.buffer.numberOfChannels} ${track.buffer.numberOfChannels === 1 ? "canal" : "canais"}`
          : target
            ? "Importe a voz ou as notas que você quer estudar."
            : "Você também pode praticar só com a faixa alvo."}
      </p>
      {target && (
        <label className="field target-mode-field">
          Tipo do alvo
          <select
            aria-label="Tipo da faixa alvo"
            value={studio.targetModeChosen ? studio.targetMode : ""}
            disabled={busy}
            onChange={(event) =>
              void studio.changeTargetMode(
                event.target.value as "monophonic" | "polyphonic",
              )
            }
          >
            <option value="" disabled>
              Escolha o conteúdo do alvo
            </option>
            <option value="monophonic">Uma voz isolada</option>
            <option value="polyphonic">
              Várias vozes/notas · experimental
            </option>
          </select>
          <small>
            {!studio.targetModeChosen
              ? "Escolha conforme o conteúdo do arquivo para usar a análise adequada."
              : studio.targetMode === "polyphonic"
                ? "Estima notas simultâneas. Depois, escolha e revise uma linha. Use um trecho curto para a primeira análise."
                : "Detecta o contorno de uma única voz. Prefira uma gravação isolada."}
            {track && " Alterar o tipo reanalisa este arquivo."}
          </small>
        </label>
      )}
      <input
        ref={fileInput}
        type="file"
        accept="audio/*,.wav,.mp3,.m4a,.ogg,.flac,.aac"
        aria-label={target ? "Selecionar voz alvo" : "Selecionar backing track"}
        disabled={busy || (target && !studio.targetModeChosen)}
        onChange={(e) => {
          const f = e.target.files?.[0];
          if (f) void studio.loadFile(role, f);
          e.target.value = "";
        }}
        className="file-picker"
      />
      <button
        className="button button-secondary"
        disabled={busy || (target && !studio.targetModeChosen)}
        onClick={() => fileInput.current?.click()}
      >
        <Upload size={15} />
        {progress !== null
          ? "Analisando…"
          : track
            ? "Trocar arquivo"
            : "Selecionar arquivo"}
      </button>
      {!target && track && (
        <button
          className="remove-backing"
          disabled={busy}
          onClick={studio.removeBacking}
        >
          <X size={13} /> Remover acompanhamento
        </button>
      )}
      {progress !== null ? (
        <div className="upload-progress" role="status">
          <div className="progress-bar">
            <span style={{ width: `${Math.max(3, progress * 100)}%` }} />
          </div>
          <span>
            {target
              ? studio.targetMode === "polyphonic"
                ? "Estimando notas simultâneas"
                : "Extraindo melodia"
              : "Preparando áudio"}{" "}
            · {Math.round(progress * 100)}%
          </span>
        </div>
      ) : (
        <span className="drop-hint">
          {track
            ? "Arquivo carregado neste navegador"
            : target && !studio.targetModeChosen
              ? "Escolha o tipo do alvo antes de carregar"
              : "ou arraste o arquivo até aqui"}
        </span>
      )}
    </section>
  );
}
