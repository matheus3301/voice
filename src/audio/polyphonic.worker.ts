import * as tf from "@tensorflow/tfjs";
import {
  BasicPitch,
  noteFramesToTime,
  outputToNotesPoly,
} from "@spotify/basic-pitch";
import { resampleMono } from "./pitch";

self.onmessage = async (
  event: MessageEvent<{
    channels: Float32Array[];
    sampleRate: number;
    modelUrl: string;
  }>,
) => {
  let model: Awaited<BasicPitch["model"]> | undefined;
  let scopeStarted = false;
  try {
    await tf.setBackend("cpu");
    await tf.ready();
    const basicPitch = new BasicPitch(event.data.modelUrl);
    model = await basicPitch.model;
    const samples = resampleMono(
      event.data.channels,
      event.data.sampleRate,
      22050,
    );
    const frames: number[][] = [];
    const onsets: number[][] = [];
    // Keep a scope alive across the serial async inference. tf.tidy cannot wrap a Promise.
    tf.engine().startScope();
    scopeStarted = true;
    await basicPitch.evaluateModel(
      samples,
      (f, o) => {
        frames.push(...f);
        onsets.push(...o);
      },
      (progress) => self.postMessage({ progress }),
    );
    const notes = noteFramesToTime(
      outputToNotesPoly(frames, onsets, 0.5, 0.3, 5),
    );
    self.postMessage({
      notes,
      diagnostics: {
        backend: tf.getBackend(),
        tensors: tf.memory().numTensors,
      },
    });
  } catch (error) {
    self.postMessage({
      error:
        error instanceof Error ? error.message : "Falha na análise polifônica.",
    });
  } finally {
    if (scopeStarted) tf.engine().endScope();
    model?.dispose();
  }
};
