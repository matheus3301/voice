import { Check, SlidersHorizontal } from "lucide-react";
import { noteName } from "../practice";
import type { useStudio } from "../useStudio";

type Studio = ReturnType<typeof useStudio>;
const selectableNotes = Array.from({ length: 51 }, (_, index) => index + 36);

// A singer reviews a tentative melody before measuring a take. The selected
// mint line stays the focus; secondary counts and native inset selects use the
// console's existing type scale, borders and 4px spacing rhythm.
export default function TargetReview({ studio: s }: { studio: Studio }) {
  const selection = s.targetSelection;
  const candidates = s.tracks.target?.notes.length ?? 0;
  const selected = s.practiceTarget?.notes.length ?? 0;
  const busy = Object.values(s.progress).some((value) => value !== null);
  const confirmed = s.targetReviewed && s.scoringReady;

  return (
    <section
      className="panel target-review"
      aria-label="Revisão do alvo experimental"
    >
      <div className="panel-heading">
        <span className="section-label">
          <SlidersHorizontal size={14} />
          REVISAR LINHA ALVO
        </span>
        <span className={`review-state ${confirmed ? "confirmed" : ""}`}>
          {confirmed ? (
            <>
              <Check size={13} />
              Revisada
            </>
          ) : (
            "Aguardando revisão"
          )}
        </span>
      </div>
      <p className="review-explanation">
        Transcrição experimental: estima notas simultâneas, sem identificar
        cantores ou naipes. A linha pode alternar entre vozes. Ouça o trecho e
        confira a seleção verde no gráfico antes de usá-la para pontuar. A
        avaliação aceita C2 a D6; candidatas fora dessa faixa ficam apenas como
        referência visual.
      </p>
      <div className="review-controls">
        <label className="field">
          Escolher a linha
          <select
            aria-label="Critério da linha alvo"
            value={selection.mode}
            disabled={busy}
            onChange={(event) =>
              s.setTargetSelection({
                ...selection,
                mode: event.target.value as "highest" | "lowest",
              })
            }
          >
            <option value="highest">Nota mais aguda disponível</option>
            <option value="lowest">Nota mais grave disponível</option>
          </select>
        </label>
        <label className="field">
          Nota mínima
          <select
            aria-label="Nota mínima da linha alvo"
            value={selection.minMidi}
            disabled={busy}
            onChange={(event) => {
              const minMidi = Number(event.target.value);
              s.setTargetSelection({
                ...selection,
                minMidi,
                maxMidi: Math.max(minMidi, selection.maxMidi),
              });
            }}
          >
            {selectableNotes.map((midi) => (
              <option key={midi} value={midi}>
                {noteName(midi)}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          Nota máxima
          <select
            aria-label="Nota máxima da linha alvo"
            value={selection.maxMidi}
            disabled={busy}
            onChange={(event) => {
              const maxMidi = Number(event.target.value);
              s.setTargetSelection({
                ...selection,
                maxMidi,
                minMidi: Math.min(maxMidi, selection.minMidi),
              });
            }}
          >
            {selectableNotes.map((midi) => (
              <option key={midi} value={midi}>
                {noteName(midi)}
              </option>
            ))}
          </select>
        </label>
      </div>
      <div className="review-summary" role="status">
        <span>
          {candidates}{" "}
          {candidates === 1 ? "nota candidata" : "notas candidatas"} ·{" "}
          {selected} {selected === 1 ? "nota" : "notas"} na linha selecionada
        </span>
        {!candidates ? (
          <p>
            Nenhuma nota candidata foi detectada. Tente outro trecho ou use uma
            voz isolada no modo monofônico.
          </p>
        ) : !selected ? (
          <p>
            Nenhuma nota nesta seleção. Amplie a faixa de notas para escolher
            uma linha.
          </p>
        ) : (
          <p>
            {confirmed
              ? "Linha confirmada. A avaliação continua experimental."
              : "A pontuação fica bloqueada até você revisar e confirmar esta linha."}
          </p>
        )}
      </div>
      <button
        className={`button ${confirmed ? "button-secondary" : "button-primary"}`}
        disabled={busy || !selected || confirmed}
        onClick={s.confirmTarget}
      >
        {confirmed && <Check size={14} />}
        {confirmed
          ? "Linha revisada para este alvo"
          : "Revisei e quero usar esta linha"}
      </button>
    </section>
  );
}
