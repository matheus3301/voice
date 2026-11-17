/// <reference types="vite/client" />
import type { NoteSegment } from "../types";
import {
  mergeBoundaryNotes,
  planAnalysisChunks,
  trimChunkNotes,
} from "./polyphonic";
import type { AnalysisChunk, BoundaryNote, ModelNote } from "./polyphonic";

export class PolyphonicAnalyzer {
  private generation = 0;
  private active: { worker: Worker; reject: (reason: Error) => void } | null =
    null;
  cancel(): void {
    this.generation++;
    const active = this.active;
    this.active = null;
    active?.reject(new Error("Análise cancelada."));
  }

  async analyze(
    buffer: AudioBuffer,
    progress?: (value: number) => void,
  ): Promise<NoteSegment[]> {
    this.cancel();
    const generation = this.generation;
    const chunks = planAnalysisChunks(buffer.duration);
    const candidates: BoundaryNote[] = [];
    progress?.(0);
    for (let index = 0; index < chunks.length; index++) {
      if (generation !== this.generation) throw new Error("Análise cancelada.");
      const chunk = chunks[index];
      const notes = await this.analyzeChunk(buffer, chunk, (value) => {
        if (generation === this.generation)
          progress?.((index + value) / chunks.length);
      });
      if (generation !== this.generation) throw new Error("Análise cancelada.");
      candidates.push(...trimChunkNotes(notes, chunk));
    }
    progress?.(1);
    return mergeBoundaryNotes(candidates);
  }

  private analyzeChunk(
    buffer: AudioBuffer,
    chunk: AnalysisChunk,
    progress: (value: number) => void,
  ): Promise<ModelNote[]> {
    return new Promise((resolve, reject) => {
      const worker = new Worker(
        new URL("./polyphonic.worker.ts", import.meta.url),
        { type: "module" },
      );
      let settled = false;
      const timeout = setTimeout(
        () =>
          fail(
            new Error(
              "A análise deste trecho excedeu 120 segundos. Tente um arquivo menor ou outro navegador.",
            ),
          ),
        120_000,
      );
      const cleanup = () => {
        clearTimeout(timeout);
        worker.terminate();
        if (this.active?.worker === worker) this.active = null;
      };
      const fail = (error: Error) => {
        if (!settled) {
          settled = true;
          cleanup();
          reject(error);
        }
      };
      this.active = { worker, reject: fail };
      worker.onmessage = (
        event: MessageEvent<{
          progress?: number;
          notes?: ModelNote[];
          error?: string;
        }>,
      ) => {
        if (settled) return;
        if (event.data.progress !== undefined)
          progress(Math.max(0, Math.min(1, event.data.progress)));
        if (event.data.error)
          fail(new Error(`Análise polifônica: ${event.data.error}`));
        else if (event.data.notes) {
          settled = true;
          cleanup();
          resolve(event.data.notes);
        }
      };
      worker.onerror = () =>
        fail(
          new Error(
            "O modelo polifônico não pôde ser executado. Verifique os assets locais e tente outro navegador.",
          ),
        );
      try {
        const start = Math.floor(chunk.start * buffer.sampleRate);
        const end = Math.ceil(chunk.end * buffer.sampleRate);
        const channels = Array.from(
          { length: Math.min(2, buffer.numberOfChannels) },
          (_, index) => buffer.getChannelData(index).slice(start, end),
        );
        const modelUrl = new URL(
          `${import.meta.env.BASE_URL}models/basic-pitch/model.json`,
          globalThis.location.href,
        ).href;
        worker.postMessage(
          { channels, sampleRate: buffer.sampleRate, modelUrl },
          channels.map((channel) => channel.buffer),
        );
      } catch (error) {
        fail(
          error instanceof Error
            ? error
            : new Error("Falha ao preparar a análise."),
        );
      }
    });
  }
}
