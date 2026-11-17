import {
  ANALYSIS_RATE,
  analyzePitch,
  createWaveform,
  resampleMono,
  segmentNotes,
} from "./pitch";

interface AnalysisRequest {
  id: number;
  channels: Float32Array[];
  sampleRate: number;
  duration: number;
  analyze: boolean;
}
self.onmessage = (event: MessageEvent<AnalysisRequest>) => {
  const { id, channels, sampleRate, duration, analyze } = event.data;
  try {
    const waveform = createWaveform(channels);
    const samples = analyze ? resampleMono(channels, sampleRate) : null;
    const pitch = samples
      ? analyzePitch(samples, ANALYSIS_RATE, (progress) => {
          self.postMessage({ id, progress });
        })
      : [];
    const notes = segmentNotes(pitch, duration);
    self.postMessage(
      { id, result: { waveform, pitch, notes } },
      { transfer: [waveform.buffer] },
    );
  } catch (error) {
    self.postMessage({
      id,
      error:
        error instanceof Error ? error.message : "Falha ao analisar áudio.",
    });
  }
};
