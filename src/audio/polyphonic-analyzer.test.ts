import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PolyphonicAnalyzer } from "./polyphonic-analyzer";

class FakePolyWorker {
  static instances: FakePolyWorker[] = [];
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onerror: (() => void) | null = null;
  postMessage = vi.fn();
  terminate = vi.fn();
  constructor() {
    FakePolyWorker.instances.push(this);
  }
  result(notes: unknown[] = []) {
    this.onmessage?.({ data: { notes } });
  }
}
function buffer(duration = 5): AudioBuffer {
  return {
    duration,
    sampleRate: 100,
    numberOfChannels: 1,
    getChannelData: () => new Float32Array(duration * 100),
  } as unknown as AudioBuffer;
}

describe("polyphonic worker lifecycle", () => {
  let analyzer: PolyphonicAnalyzer;
  beforeEach(() => {
    vi.useFakeTimers();
    FakePolyWorker.instances = [];
    vi.stubGlobal("Worker", FakePolyWorker);
    vi.stubGlobal("location", { href: "https://studio.test/" });
    analyzer = new PolyphonicAnalyzer();
  });
  afterEach(() => {
    analyzer.cancel();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("terminates each worker after success and returns compact simultaneous candidates", async () => {
    const pending = analyzer.analyze(buffer());
    const worker = FakePolyWorker.instances[0];
    expect(worker.postMessage.mock.calls[0][0].modelUrl).toBe(
      "https://studio.test/models/basic-pitch/model.json",
    );
    worker.result([
      {
        startTimeSeconds: 1,
        durationSeconds: 2,
        pitchMidi: 60,
        amplitude: 0.8,
      },
      {
        startTimeSeconds: 1,
        durationSeconds: 2,
        pitchMidi: 64,
        amplitude: 0.5,
      },
    ]);
    await expect(pending).resolves.toEqual([
      { start: 1, end: 3, midi: 60 },
      { start: 1, end: 3, midi: 64 },
    ]);
    expect(worker.terminate).toHaveBeenCalledOnce();
  });

  it("uses a fresh worker for every chunk and reports aggregate progress", async () => {
    const progress = vi.fn();
    const pending = analyzer.analyze(buffer(45), progress);
    const first = FakePolyWorker.instances[0];
    expect(first.postMessage.mock.calls[0][0].channels[0].length).toBe(2050);
    first.result();
    await Promise.resolve();
    const second = FakePolyWorker.instances[1];
    expect(first.terminate).toHaveBeenCalledOnce();
    expect(second.postMessage.mock.calls[0][0].channels[0].length).toBe(2100);
    second.onmessage?.({ data: { progress: 0.5 } });
    expect(progress).toHaveBeenLastCalledWith(0.5);
    second.result();
    await Promise.resolve();
    FakePolyWorker.instances[2].result();
    await expect(pending).resolves.toEqual([]);
    expect(
      FakePolyWorker.instances.every(
        (worker) => worker.terminate.mock.calls.length === 1,
      ),
    ).toBe(true);
    expect(progress).toHaveBeenLastCalledWith(1);
  });

  it("cancels a running worker and ignores its late result", async () => {
    const pending = analyzer.analyze(buffer());
    const rejected = expect(pending).rejects.toThrow("cancelada");
    const worker = FakePolyWorker.instances[0];
    analyzer.cancel();
    worker.result([
      { startTimeSeconds: 1, durationSeconds: 2, pitchMidi: 60, amplitude: 1 },
    ]);
    await rejected;
    expect(worker.terminate).toHaveBeenCalledOnce();
  });

  it("times out a stalled chunk and releases its worker", async () => {
    const pending = analyzer.analyze(buffer());
    const rejected = expect(pending).rejects.toThrow("120 segundos");
    vi.advanceTimersByTime(120000);
    await rejected;
    expect(FakePolyWorker.instances[0].terminate).toHaveBeenCalledOnce();
  });

  it("a new analysis cancels the old one without losing the new result", async () => {
    const old = analyzer.analyze(buffer());
    const oldRejected = expect(old).rejects.toThrow("cancelada");
    const next = analyzer.analyze(buffer());
    FakePolyWorker.instances[0].result();
    FakePolyWorker.instances[1].result([
      { startTimeSeconds: 1, durationSeconds: 2, pitchMidi: 67, amplitude: 1 },
    ]);
    await oldRejected;
    await expect(next).resolves.toEqual([{ start: 1, end: 3, midi: 67 }]);
  });

  it("reports worker failure and releases its resources", async () => {
    const pending = analyzer.analyze(buffer());
    FakePolyWorker.instances[0].onerror?.();
    await expect(pending).rejects.toThrow("não pôde ser executado");
    expect(FakePolyWorker.instances[0].terminate).toHaveBeenCalledOnce();
  });
});
