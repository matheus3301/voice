import { Activity, ArrowLeft, Headphones, Mic, Settings2 } from "lucide-react";
import type { useStudio } from "../useStudio";

type Studio = ReturnType<typeof useStudio>;

interface Props {
  studio: Studio;
  onOpenImport: () => void;
  output: string;
  onOutputChange: (value: string) => void;
}

export default function AudioSettings({
  studio: s,
  onOpenImport,
  output,
  onOutputChange,
}: Props) {
  const rms = s.currentPitch?.rms ?? 0;
  const inputs = s.devices.filter((d) => d.kind === "audioinput"),
    outputs = s.devices.filter((d) => d.kind === "audiooutput");
  const canOutput =
    typeof AudioContext !== "undefined" &&
    "setSinkId" in AudioContext.prototype;
  return (
    <aside className="side-panel">
      <section className="panel input-card">
        <div className="panel-heading">
          <span className="section-label">ENTRADA DE ÁUDIO</span>
          <Mic size={16} />
        </div>
        <div className={`input-status ${s.micActive ? "connected" : ""}`}>
          <span className="status-dot" />
          {s.micBusy
            ? "Conectando…"
            : s.micActive
              ? "Microfone conectado"
              : "Microfone desligado"}
        </div>
        <button
          className={`button connection-button ${s.micActive ? "button-secondary" : "button-primary"}`}
          disabled={s.micBusy}
          onClick={() =>
            s.micActive ? s.disconnectMic() : void s.connectMic()
          }
        >
          <Mic size={15} />
          {s.micActive
            ? "Desligar entrada"
            : s.micBusy
              ? "Conectando…"
              : "Ativar entrada"}
        </button>
        {s.micBusy && (
          <div className="mic-permission-hint">
            <p>Permita o microfone no navegador para conectar.</p>
            <button
              className="button button-secondary"
              onClick={s.disconnectMic}
            >
              Cancelar conexão
            </button>
          </div>
        )}
        <label className="field">
          Dispositivo
          <select
            aria-label="Dispositivo de entrada"
            value={s.deviceId}
            disabled={s.micBusy}
            onChange={(e) => s.changeDevice(e.target.value)}
          >
            <option value="">Padrão do sistema</option>
            {inputs
              .filter((d) => d.deviceId && d.deviceId !== "default")
              .map((d, i) => (
                <option key={d.deviceId} value={d.deviceId}>
                  {d.label || `Entrada ${i + 1}`}
                </option>
              ))}
          </select>
        </label>
        <div className="field-row">
          <label className="field">
            Canal
            <select
              aria-label="Canal de entrada"
              value={s.channel}
              disabled={!s.micActive || s.micBusy}
              onChange={(e) => s.changeChannel(Number(e.target.value))}
            >
              {Array.from(
                { length: Math.max(1, s.diagnostics?.channels ?? 1) },
                (_, i) => (
                  <option key={i} value={i}>
                    Canal {i + 1}
                  </option>
                ),
              )}
            </select>
          </label>
          <label className="field">
            Ganho <span className="field-value">{s.inputGain.toFixed(1)}×</span>
            <input
              aria-label="Ganho de entrada"
              type="range"
              min={0.25}
              max={3}
              step={0.05}
              value={s.inputGain}
              onChange={(e) => s.setInputGain(Number(e.target.value))}
            />
          </label>
        </div>
        <div className="meter-label">
          <span>Nível de entrada</span>
          <span className={rms > 0.7 ? "clip" : ""}>
            {rms > 0.001
              ? `${Math.round(20 * Math.log10(rms))} dBFS`
              : "−∞ dBFS"}
          </span>
        </div>
        <div className="meter" aria-label="Medidor de entrada">
          <div
            className={`meter-fill ${rms > 0.7 ? "clipping" : ""}`}
            style={{
              width: `${Math.max(0, Math.min(100, ((20 * Math.log10(Math.max(0.0001, rms)) + 60) / 60) * 100))}%`,
            }}
          />
        </div>
        <div className="toggle-row">
          <div>
            <Headphones size={16} />
            <span>Retorno do microfone</span>
          </div>
          <button
            className={`toggle ${s.monitor ? "on" : ""}`}
            role="switch"
            aria-checked={s.monitor}
            aria-label="Retorno do microfone"
            disabled={!s.micActive}
            onClick={() => s.setMonitor(!s.monitor)}
          >
            <span />
          </button>
        </div>
        {s.monitor && (
          <p className="warning">
            Use fones para evitar realimentação. Se sua interface já tem retorno
            direto, desligue este retorno.
          </p>
        )}
        <details className="settings-details">
          <summary>
            <Settings2 size={14} />
            Configuração de áudio
          </summary>
          <label className="field">
            Janela de análise
            <select
              aria-label="Janela de análise"
              value={s.windowSize}
              disabled={s.micBusy}
              onChange={(e) => s.changeWindow(Number(e.target.value))}
            >
              <option value={2048}>2048 amostras · resposta rápida</option>
              <option value={4096}>4096 amostras · equilibrada</option>
              <option value={8192}>8192 amostras · notas graves</option>
            </select>
          </label>
          <label className="field">
            Compensação <span className="field-value">{s.latency} ms</span>
            <input
              aria-label="Compensação de latência"
              type="range"
              min={-250}
              max={500}
              step={5}
              value={s.latency}
              onChange={(e) => s.setLatency(Number(e.target.value))}
            />
            <small>Valores positivos alinham uma entrada atrasada.</small>
          </label>
          <label className="field">
            Saída de áudio
            <select
              aria-label="Dispositivo de saída"
              value={output}
              disabled={!canOutput}
              onChange={(e) => {
                onOutputChange(e.target.value);
                void s.setOutput(e.target.value);
              }}
            >
              <option value="">Padrão do sistema</option>
              {outputs
                .filter((d) => d.deviceId && d.deviceId !== "default")
                .map((d, i) => (
                  <option value={d.deviceId} key={d.deviceId}>
                    {d.label || `Saída ${i + 1}`}
                  </option>
                ))}
            </select>
          </label>
          <p className="audio-limits">
            O navegador controla o buffer de áudio. Ajuste o buffer do driver no
            sistema. Canais e saída dependem do suporte do navegador.
          </p>
          {s.diagnostics && (
            <div className="diagnostics">
              <span>
                Sample rate
                <strong>
                  {(s.diagnostics.sampleRate / 1000).toFixed(1)} kHz
                </strong>
              </span>
              <span>
                Latência do grafo
                <strong>
                  {(s.diagnostics.baseLatency * 1000).toFixed(1)} ms
                </strong>
              </span>
              <span>
                Latência de saída
                <strong>
                  {s.diagnostics.outputLatency > 0
                    ? `${(s.diagnostics.outputLatency * 1000).toFixed(1)} ms`
                    : "Não informada"}
                </strong>
              </span>
            </div>
          )}
        </details>
      </section>
      <section className="panel practice-card">
        <div className="panel-heading">
          <span className="section-label">TENTATIVA</span>
          <Activity size={16} />
        </div>
        <label className="field">
          Tolerância de afinação
          <select
            aria-label="Tolerância de afinação"
            value={s.tolerance}
            onChange={(e) => s.setTolerance(Number(e.target.value))}
          >
            <option value={25}>Precisa · ±25 cents</option>
            <option value={50}>Padrão · ±50 cents</option>
            <option value={75}>Ampla · ±75 cents</option>
          </select>
        </label>
        <p>
          Acerto é o tempo no alvo dentro da tolerância. Cobertura é quanto do
          alvo você cantou. Silêncio durante o alvo conta como erro.
        </p>
        <div className="attempt-duration">
          <span>Alvo avaliado</span>
          <strong>{s.stats.targetSeconds.toFixed(1)} s</strong>
        </div>
        <small>
          Voltar, posicionar ou repetir um loop inicia uma nova tentativa.
        </small>
      </section>
      <button className="back-to-files" onClick={onOpenImport}>
        <ArrowLeft size={14} />
        Trocar arquivos da sessão
      </button>
    </aside>
  );
}
