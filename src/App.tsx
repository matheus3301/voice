import { useEffect, useState } from "react";
import {
  AudioLines,
  ChevronRight,
  CircleHelp,
  Headphones,
  Pause,
  Play,
  Repeat2,
  RotateCcw,
  SkipBack,
  Upload,
  Volume2,
  VolumeX,
  X,
  ZoomIn,
} from "lucide-react";
import { useStudio } from "./useStudio";
import PianoRoll from "./components/PianoRoll";
import Waveform from "./components/Waveform";
import { formatTime, intervalName, noteName, pitchAt } from "./practice";
import AudioSettings from "./components/AudioSettings";
import UploadSlot from "./components/UploadSlot";

// Visual brief: a musician at the desk, focused on following the target.
// The piano roll owns the canvas; quiet graphite panels hold mint target and amber input.
// System sans at 13px, monospaced time, 4px rhythm and 16px panel insets keep this a tool.

export default function App() {
  const s = useStudio();
  const [workspace, setWorkspace] = useState(false);
  const [demoPending, setDemoPending] = useState(false);
  const [windowSeconds, setWindowSeconds] = useState(12);
  const [showHelp, setShowHelp] = useState(false);
  const [output, setOutput] = useState("");
  const bothReady = !!s.tracks.backing && !!s.tracks.target;
  const busy = Object.values(s.progress).some((p) => p !== null);
  useEffect(() => {
    if (demoPending && bothReady && !busy) {
      setWorkspace(true);
      setDemoPending(false);
    }
  }, [demoPending, bothReady, busy]);
  const startDemo = () => {
    setDemoPending(true);
    void s.demo();
  };
  const currentTarget = s.tracks.target
    ? pitchAt(
        s.tracks.target.pitch,
        s.micActive && s.playing && s.currentPitch
          ? s.currentPitch.time
          : s.time,
      )
    : null;
  const sung =
    s.micActive &&
    s.currentPitch &&
    s.currentPitch.frequency > 0 &&
    s.currentPitch.confidence >= 0.7
      ? s.currentPitch
      : null;
  const cents =
    sung && currentTarget ? (sung.midi - currentTarget.midi) * 100 : null;
  const notes = s.tracks.target?.notes ?? [];
  const upcoming = notes.find((n) => n.start > s.time + 0.08);
  const currentNote =
    notes.find((n) => n.start <= s.time && n.end >= s.time) ??
    [...notes].reverse().find((n) => n.end < s.time);
  const nextInterval =
    upcoming && currentNote
      ? intervalName(upcoming.midi - currentNote.midi)
      : null;
  const hasScore = s.stats.targetSeconds >= 0.1;
  const sessionName =
    s.tracks.target?.name.replace(/\.[^.]+$/, "") ?? "Nova sessão";
  const openImport = () => {
    if (s.playing) void s.togglePlayback();
    s.disconnectMic();
    setWorkspace(false);
  };

  return (
    <div className="shell">
      <header className="topbar">
        <div className="brand">
          <span className="brand-icon">
            <AudioLines size={19} />
          </span>
          <strong>Estúdio vocal</strong>
          <span className="version-tag">v0.1</span>
        </div>
        <div className="header-session">
          <span className="status-dot" />
          {workspace ? sessionName : "Nova sessão"}
        </div>
        <div className="header-actions">
          <span className="local-label">Processamento local</span>
          <button
            className={`icon-button ${showHelp ? "active" : ""}`}
            aria-label="Ajuda e atalhos"
            title="Ajuda e atalhos"
            onClick={() => setShowHelp((v) => !v)}
          >
            <CircleHelp size={18} />
          </button>
          {workspace && (
            <button
              className="button button-secondary small"
              onClick={openImport}
            >
              <Upload size={14} />
              Arquivos
            </button>
          )}
        </div>
      </header>
      {s.error && (
        <div className="error-banner" role="alert">
          <span>{s.error}</span>
          <button
            className="icon-button"
            aria-label="Fechar erro"
            onClick={() => s.setError("")}
          >
            <X size={16} />
          </button>
        </div>
      )}
      {showHelp && (
        <aside className="help-panel">
          <div>
            <strong>Controles da sessão</strong>
            <button
              className="icon-button"
              aria-label="Fechar ajuda"
              onClick={() => setShowHelp(false)}
            >
              <X size={16} />
            </button>
          </div>
          <p>
            <kbd>Espaço</kbd> Reproduzir / pausar <span>·</span>{" "}
            <kbd>Enter</kbd> Voltar ao início
          </p>
          <p>
            Clique na forma de onda ou no gráfico para posicionar. Use A e B
            para repetir um trecho. As teclas preservam o comportamento de
            campos e botões.
          </p>
          <p>
            Use uma trilha alvo com uma única voz. Instrumentos misturados ou
            várias vozes simultâneas podem causar erros na detecção de notas.
          </p>
        </aside>
      )}
      {!workspace ? (
        <main className="upload-screen">
          <div className="upload-heading">
            <span className="section-label">PREPARAR SESSÃO</span>
            <h1>Carregue suas duas faixas.</h1>
            <p>
              A base toca junto. A voz alvo vira o mapa de notas para você
              seguir.
            </p>
          </div>
          <div className="upload-grid">
            <UploadSlot role="backing" studio={s} />
            <UploadSlot role="target" studio={s} />
          </div>
          <div className="upload-footer">
            <div>
              <Headphones size={17} />
              <span>
                Áudios ficam neste navegador.
                <br />
                <small>
                  Até 80 MB e 15 min por faixa. Formatos dependem do navegador.
                </small>
              </span>
            </div>
            <button
              className="button button-primary"
              disabled={!bothReady || busy}
              onClick={() => setWorkspace(true)}
            >
              Abrir bancada
              <ChevronRight size={17} />
            </button>
          </div>
          <div className="demo-option">
            <span>Quer testar os controles primeiro?</span>
            <button disabled={busy} onClick={startDemo}>
              {busy && demoPending
                ? "Preparando demonstração…"
                : "Abrir demonstração de 24 segundos"}
              <ChevronRight size={14} />
            </button>
            <small>
              Melodia e acompanhamento sintetizados; a entrada de voz usa seu
              microfone.
            </small>
          </div>
        </main>
      ) : (
        <>
          <section className="transport" aria-label="Transporte">
            <div className="transport-main">
              <div className="transport-buttons">
                <button
                  className="icon-button"
                  aria-label="Voltar ao início"
                  title="Voltar ao início · Enter"
                  onClick={() => s.seek(0)}
                >
                  <SkipBack size={19} />
                </button>
                <button
                  className="play-button"
                  aria-label={s.playing ? "Pausar" : "Reproduzir"}
                  title="Reproduzir / pausar · Espaço"
                  disabled={!s.duration}
                  onClick={() => void s.togglePlayback()}
                >
                  {s.playing ? (
                    <Pause size={21} fill="currentColor" />
                  ) : (
                    <Play size={21} fill="currentColor" />
                  )}
                </button>
                <button
                  className={`icon-button ${s.loop.enabled ? "active" : ""}`}
                  aria-label="Repetir trecho"
                  aria-pressed={s.loop.enabled}
                  title="Loop A–B"
                  onClick={() =>
                    s.setLoop({ ...s.loop, enabled: !s.loop.enabled })
                  }
                >
                  <Repeat2 size={20} />
                </button>
              </div>
              <div className="time-display">
                {formatTime(s.time, true)}
                <span className="time-total"> / {formatTime(s.duration)}</span>
              </div>
              <span className={`play-state ${s.playing ? "running" : ""}`}>
                <span className="status-dot" />
                {s.playing ? "Reproduzindo" : "Em pausa"}
              </span>
            </div>
            <div className="transport-loop">
              <span className="section-label">LOOP</span>
              <label>
                A{" "}
                <input
                  aria-label="Início do loop em segundos"
                  type="number"
                  min={0}
                  max={s.duration}
                  step="0.1"
                  value={Number(s.loop.start.toFixed(1))}
                  onChange={(e) =>
                    s.setLoop({ ...s.loop, start: Number(e.target.value) })
                  }
                />
              </label>
              <button
                className="loop-set"
                title="Definir início aqui"
                onClick={() =>
                  s.setLoop({
                    ...s.loop,
                    start: Math.min(s.time, s.loop.end - 0.1),
                  })
                }
              >
                A aqui
              </button>
              <span className="loop-divider">—</span>
              <label>
                B{" "}
                <input
                  aria-label="Fim do loop em segundos"
                  type="number"
                  min={0}
                  max={s.duration}
                  step="0.1"
                  value={Number(s.loop.end.toFixed(1))}
                  onChange={(e) =>
                    s.setLoop({ ...s.loop, end: Number(e.target.value) })
                  }
                />
              </label>
              <button
                className="loop-set"
                title="Definir fim aqui"
                onClick={() =>
                  s.setLoop({
                    ...s.loop,
                    end: Math.max(s.time, s.loop.start + 0.1),
                  })
                }
              >
                B aqui
              </button>
            </div>
          </section>
          <main className="workspace">
            <div className="main-column">
              <section
                className="panel tracks-panel"
                aria-label="Mixer de faixas"
              >
                <div className="panel-heading">
                  <span className="section-label">FAIXAS</span>
                  <span className="subtle">Clique na onda para posicionar</span>
                </div>
                <div className="track-list">
                  {(["backing", "target"] as const).map((role) => (
                    <div className={`track-row ${role}`} key={role}>
                      <div className="track-icon">
                        <AudioLines size={18} />
                      </div>
                      <div className="track-details">
                        <span className="track-role">
                          {role === "backing" ? "Backing track" : "Voz alvo"}
                        </span>
                        <span
                          className="track-name"
                          title={s.tracks[role]?.name}
                        >
                          {s.tracks[role]?.name ?? "Sem arquivo"}
                        </span>
                      </div>
                      <Waveform
                        track={s.tracks[role]}
                        time={s.time}
                        duration={s.duration}
                        onSeek={s.seek}
                        color={role === "backing" ? "#87a5b8" : "#a8e4ca"}
                        loop={s.loop}
                      />
                      <div className="track-controls">
                        <button
                          className={`mute-button ${s.muted[role] ? "muted" : ""}`}
                          aria-label={`${s.muted[role] ? "Ativar" : "Silenciar"} ${role === "backing" ? "backing track" : "voz alvo"}`}
                          aria-pressed={s.muted[role]}
                          onClick={() => s.toggleMute(role)}
                        >
                          {s.muted[role] ? (
                            <VolumeX size={16} />
                          ) : (
                            <Volume2 size={16} />
                          )}
                        </button>
                        <input
                          type="range"
                          aria-label={`Volume ${role === "backing" ? "backing track" : "voz alvo"}`}
                          min={0}
                          max={1}
                          step={0.01}
                          value={s.volumes[role]}
                          onChange={(e) =>
                            s.setVolume(role, Number(e.target.value))
                          }
                        />
                        <span className="track-volume">
                          {s.muted[role]
                            ? "−∞"
                            : `${Math.round(s.volumes[role] * 100)}%`}
                        </span>
                      </div>
                    </div>
                  ))}
                </div>
              </section>
              <section className="panel roll-panel">
                <div className="roll-header">
                  <div>
                    <span className="section-label">AFINAÇÃO</span>
                    <div className="roll-legend">
                      <span>
                        <i className="target-key" />
                        Voz alvo
                      </span>
                      <span>
                        <i className="voice-key" />
                        Sua voz
                      </span>
                    </div>
                  </div>
                  <div className="roll-tools">
                    {nextInterval && (
                      <span className="interval-label">
                        Próximo <strong>{nextInterval}</strong>
                      </span>
                    )}
                    <label className="zoom-label">
                      <ZoomIn size={15} />
                      <select
                        aria-label="Janela de tempo do gráfico"
                        value={windowSeconds}
                        onChange={(e) =>
                          setWindowSeconds(Number(e.target.value))
                        }
                      >
                        <option value={6}>6 s</option>
                        <option value={12}>12 s</option>
                        <option value={24}>24 s</option>
                      </select>
                    </label>
                  </div>
                </div>
                <PianoRoll
                  track={s.tracks.target}
                  time={s.time}
                  live={s.live}
                  windowSeconds={windowSeconds}
                  onSeek={s.seek}
                  isPlaying={s.playing}
                />
                <div className="roll-caption">
                  <span>
                    {s.micActive
                      ? "Entrada ativa · cante acompanhando as notas verdes"
                      : "Ative a entrada de áudio para visualizar sua voz"}
                  </span>
                  <span>A4 = 440 Hz · alvo monofônico</span>
                </div>
              </section>
              <section
                className="stats-strip"
                aria-label="Feedback da tentativa"
              >
                <div className="stat">
                  <span className="stat-label">SUA NOTA</span>
                  <strong className="stat-value voice-value">
                    {noteName(sung?.midi)}
                  </strong>
                  <small>
                    {sung
                      ? `${sung.frequency.toFixed(1)} Hz`
                      : s.micActive
                        ? "Aguardando voz"
                        : "Entrada desligada"}
                  </small>
                </div>
                <div className="stat">
                  <span className="stat-label">NOTA ALVO</span>
                  <strong className="stat-value target-value">
                    {noteName(currentTarget?.midi)}
                  </strong>
                  <small>
                    {currentTarget ? "No cursor" : "Pausa na melodia"}
                  </small>
                </div>
                <div className="stat tuning-stat">
                  <span className="stat-label">DESVIO</span>
                  <strong
                    className={`stat-value ${cents !== null && Math.abs(cents) <= s.tolerance ? "in-tune" : ""}`}
                  >
                    {cents === null
                      ? "—"
                      : `${cents > 0 ? "+" : ""}${Math.round(cents)}`}
                    <span>{cents !== null ? " ¢" : ""}</span>
                  </strong>
                  <small>
                    {cents === null
                      ? "Cante durante o alvo"
                      : Math.abs(cents) <= s.tolerance
                        ? "Dentro da tolerância"
                        : cents > 0
                          ? "Acima do alvo"
                          : "Abaixo do alvo"}
                  </small>
                  <div className="tuning-meter">
                    <i
                      style={{
                        left: `${cents === null ? 50 : Math.max(0, Math.min(100, 50 + cents / 4))}%`,
                        opacity: cents === null ? 0.2 : 1,
                      }}
                    />
                  </div>
                </div>
                <div className="stat score-stat">
                  <span className="stat-label">ACERTO</span>
                  <strong className="stat-value">
                    {hasScore ? Math.round(s.stats.score) : "—"}
                    <span>{hasScore ? "%" : ""}</span>
                  </strong>
                  <small>
                    {hasScore
                      ? `${Math.round(s.stats.coverage)}% de cobertura`
                      : "Toque e cante para pontuar"}
                  </small>
                </div>
                <button
                  className="icon-button reset-score"
                  aria-label="Limpar tentativa"
                  title="Limpar tentativa"
                  onClick={s.resetAttempt}
                >
                  <RotateCcw size={16} />
                </button>
              </section>
            </div>
            <AudioSettings
              studio={s}
              onOpenImport={openImport}
              output={output}
              onOutputChange={setOutput}
            />
          </main>
          <footer className="app-footer">
            <div>
              <span className="status-dot" />
              Áudio local<span className="footer-separator">/</span>
              {s.tracks.target?.pitch.length ?? 0} frames de melodia
            </div>
            <div>
              <kbd>Espaço</kbd> Play / pause{" "}
              <span className="footer-separator">·</span>
              <kbd>Enter</kbd> Início
            </div>
          </footer>
        </>
      )}
    </div>
  );
}
