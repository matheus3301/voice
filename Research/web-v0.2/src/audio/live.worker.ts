import { ANALYSIS_RATE, detectPitch, resampleMono } from "./pitch";
self.onmessage = (
  event: MessageEvent<{
    samples: Float32Array;
    sampleRate: number;
    time: number;
    generation: number;
    transportGeneration: number;
    loopIteration: number;
  }>,
) => {
  const {
    samples,
    sampleRate,
    time,
    generation,
    transportGeneration,
    loopIteration,
  } = event.data;
  const mono =
    sampleRate > ANALYSIS_RATE ? resampleMono([samples], sampleRate) : samples;
  const frame = detectPitch(mono, Math.min(sampleRate, ANALYSIS_RATE), time);
  self.postMessage({ frame, generation, transportGeneration, loopIteration });
};
