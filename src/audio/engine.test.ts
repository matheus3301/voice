import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AudioEngine } from "./engine";
import type { AudioTrack } from "../types";

class FakeSource {
  buffer: AudioBuffer | null = null;
  onended: (() => void) | null = null;
  connect = vi.fn();
  disconnect = vi.fn();
  start = vi.fn();
  stop = vi.fn();
}
class FakeNode {
  connect = vi.fn();
  disconnect = vi.fn();
}
class FakeGain extends FakeNode {
  gain = { value: 0, setTargetAtTime: vi.fn() };
}
class FakeCapture extends FakeNode {
  static latest: FakeCapture;
  port: {
    onmessage:
      | ((event: {
          data: {
            samples: Float32Array;
            sampleRate: number;
            centerTime: number;
          };
        }) => void)
      | null;
  } = { onmessage: null };
  constructor() {
    super();
    FakeCapture.latest = this;
  }
}
class FakeWorker {
  static latest: FakeWorker;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onerror: (() => void) | null = null;
  postMessage = vi.fn();
  terminate = vi.fn();
  constructor() {
    FakeWorker.latest = this;
  }
}
class FakeContext {
  static latest: FakeContext;
  currentTime = 0;
  sampleRate = 48000;
  baseLatency = 0.01;
  outputLatency = 0.02;
  destination = {};
  sources: FakeSource[] = [];
  gains: FakeGain[] = [];
  inputNodes: FakeNode[] = [];
  audioWorklet = { addModule: vi.fn(async () => {}) };
  resume = vi.fn(async () => {});
  close = vi.fn(async () => {});
  decodeAudioData = vi.fn(
    async (_data: ArrayBuffer) =>
      ({
        duration: 1,
        numberOfChannels: 1,
        sampleRate: 48000,
        getChannelData: () => new Float32Array(48000),
      }) as unknown as AudioBuffer,
  );
  constructor() {
    FakeContext.latest = this;
  }
  createGain() {
    const gain = new FakeGain();
    this.gains.push(gain);
    return gain;
  }
  createBufferSource() {
    const source = new FakeSource();
    this.sources.push(source);
    return source;
  }
  createMediaStreamSource() {
    const node = new FakeNode();
    this.inputNodes.push(node);
    return node;
  }
  createChannelSplitter() {
    const node = new FakeNode();
    this.inputNodes.push(node);
    return node;
  }
}

function track(duration: number): AudioTrack {
  return {
    name: "test",
    buffer: { duration } as AudioBuffer,
    waveform: new Float32Array(),
    pitch: [],
    notes: [],
  };
}

describe("Web Audio transport resources", () => {
  let engine: AudioEngine;
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("AudioContext", FakeContext);
    engine = new AudioEngine();
    engine.setTrack("backing", track(8));
    engine.setTrack("target", track(12));
  });
  afterEach(() => {
    engine.dispose();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("starts both tracks from the same clock and leaves the shorter track silent at its end", async () => {
    await engine.play(3);
    const context = FakeContext.latest;
    expect(context.sources).toHaveLength(2);
    expect(context.sources[0].start).toHaveBeenCalledWith(0.02, 3, 5);
    expect(context.sources[1].start).toHaveBeenCalledWith(0.02, 3, 9);
    expect(engine.duration).toBe(12);
    context.currentTime = 4.02;
    expect(engine.getPosition()).toBeCloseTo(7);
    expect(engine.pause()).toBeCloseTo(7);
    expect(
      context.sources.every((source) => source.stop.mock.calls.length === 1),
    ).toBe(true);
    expect(engine.isPlaying).toBe(false);
  });

  it("seek stops old sources before resuming at the new offset", async () => {
    await engine.play(0);
    const context = FakeContext.latest;
    const oldSources = [...context.sources];
    engine.seek(6);
    await Promise.resolve();
    expect(
      oldSources.every((source) => source.stop.mock.calls.length === 1),
    ).toBe(true);
    expect(context.sources).toHaveLength(4);
    expect(context.sources[2].start).toHaveBeenCalledWith(0.02, 6, 2);
    expect(context.sources[3].start).toHaveBeenCalledWith(0.02, 6, 6);
  });

  it("schedules a loop ahead at the exact audio-clock boundary", async () => {
    const onLoop = vi.fn();
    engine.onLoop = onLoop;
    engine.setLoop({ start: 4, end: 8, enabled: true });
    await engine.play(5);
    const context = FakeContext.latest;
    expect(context.sources[0].start).toHaveBeenCalledWith(0.02, 5, 3);
    context.currentTime = 2.9;
    vi.advanceTimersByTime(25);
    expect(context.sources[2].start).toHaveBeenCalledWith(3.02, 4, 4);
    context.currentTime = 3.07;
    vi.advanceTimersByTime(25);
    expect(engine.getPosition()).toBeCloseTo(4.05);
    expect(onLoop).toHaveBeenCalledOnce();
  });

  it("recovers a suspended-tab scheduler without stacking missed loops", async () => {
    engine.setLoop({ start: 4, end: 8, enabled: true });
    await engine.play(5);
    const context = FakeContext.latest;
    context.currentTime = 10;
    vi.advanceTimersByTime(25);
    expect(context.sources).toHaveLength(4);
    expect(context.sources[2].start.mock.calls[0][0]).toBeCloseTo(10.02);
    expect(context.sources[2].start.mock.calls[0][1]).toBeCloseTo(7);
  });

  it("ends at the longest track and closes resources on disposal", async () => {
    const onEnded = vi.fn();
    engine.onEnded = onEnded;
    await engine.play(0);
    const context = FakeContext.latest;
    context.currentTime = 12.1;
    vi.advanceTimersByTime(25);
    expect(engine.isPlaying).toBe(false);
    expect(engine.getPosition()).toBe(12);
    expect(onEnded).toHaveBeenCalledOnce();
    engine.dispose();
    expect(context.close).toHaveBeenCalledOnce();
  });

  it("removes a track and clamps the playhead to the remaining duration", async () => {
    await engine.play(10);
    engine.removeTrack("target");
    expect(engine.duration).toBe(8);
    expect(engine.getPosition()).toBe(8);
    expect(engine.isPlaying).toBe(false);
    engine.removeTrack("backing");
    expect(engine.duration).toBe(0);
    expect(engine.getPosition()).toBe(0);
  });

  it("does not launch analysis when decoding finishes after cancellation", async () => {
    await engine.getDiagnostics();
    let finishDecode!: (value: AudioBuffer) => void;
    FakeContext.latest.decodeAudioData.mockImplementation(
      () =>
        new Promise<AudioBuffer>((resolve) => {
          finishDecode = resolve;
        }),
    );
    const file = {
      name: "voice.wav",
      size: 20,
      arrayBuffer: async () => new ArrayBuffer(20),
    } as File;
    const pending = engine.loadFile(file, "target");
    await Promise.resolve();
    engine.cancelAnalysis();
    finishDecode({ duration: 1, numberOfChannels: 1 } as AudioBuffer);
    await expect(pending).rejects.toThrow("cancelada");
  });
});

function microphoneStream(channels = 2) {
  const audioTrack = {
    label: "Test interface",
    getSettings: () => ({ channelCount: channels }),
    stop: vi.fn(),
    onended: null as (() => void) | null,
  };
  const stream = {
    getTracks: () => [audioTrack],
    getAudioTracks: () => [audioTrack],
  } as unknown as MediaStream;
  return { stream, audioTrack };
}

describe("microphone resource lifecycle", () => {
  let engine: AudioEngine;
  let getUserMedia: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    vi.stubGlobal("AudioContext", FakeContext);
    vi.stubGlobal("AudioWorkletNode", FakeCapture);
    vi.stubGlobal("Worker", FakeWorker);
    getUserMedia = vi.fn();
    vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });
    engine = new AudioEngine();
  });
  afterEach(() => {
    engine.dispose();
    vi.unstubAllGlobals();
  });

  it("starts monitoring at zero and disconnects the stream, graph and worker", async () => {
    const { stream, audioTrack } = microphoneStream();
    getUserMedia.mockResolvedValue(stream);
    const diagnostics = await engine.connectMic("device-1", 1, 4096, vi.fn());
    const context = FakeContext.latest;
    const capture = FakeCapture.latest;
    const worker = FakeWorker.latest;
    expect(diagnostics).toMatchObject({
      sampleRate: 48000,
      channels: 2,
      deviceLabel: "Test interface",
    });
    expect(getUserMedia).toHaveBeenCalledWith({
      audio: {
        deviceId: { exact: "device-1" },
        channelCount: { ideal: 2 },
        echoCancellation: false,
        noiseSuppression: false,
        autoGainControl: false,
      },
    });
    expect(context.inputNodes[1].connect).toHaveBeenCalledWith(
      context.gains[2],
      1,
      0,
    );
    const monitor = context.gains[3];
    expect(monitor.gain.value).toBe(0);
    engine.setMonitor(true, 0.4);
    expect(monitor.gain.setTargetAtTime).toHaveBeenCalledWith(0.4, 0, 0.01);
    engine.disconnectMic();
    expect(audioTrack.stop).toHaveBeenCalledOnce();
    expect(audioTrack.onended).toBeNull();
    expect(
      context.inputNodes.every(
        (node) => node.disconnect.mock.calls.length === 1,
      ),
    ).toBe(true);
    expect(capture.disconnect).toHaveBeenCalledOnce();
    expect(capture.port.onmessage).toBeNull();
    expect(worker.terminate).toHaveBeenCalledOnce();
  });

  it("closes a granted stream when the requested channel is unavailable", async () => {
    const { stream, audioTrack } = microphoneStream(1);
    getUserMedia.mockResolvedValue(stream);
    await expect(engine.connectMic("", 1, 4096, vi.fn())).rejects.toThrow(
      "oferece 1 canal",
    );
    expect(audioTrack.stop).toHaveBeenCalledOnce();
  });

  it("closes a stream if the AudioWorklet fails to initialize", async () => {
    const { stream, audioTrack } = microphoneStream();
    getUserMedia.mockResolvedValue(stream);
    await engine.getDiagnostics();
    FakeContext.latest.audioWorklet.addModule.mockRejectedValue(
      new Error("worklet unavailable"),
    );
    await expect(engine.connectMic("", 0, 4096, vi.fn())).rejects.toThrow(
      "worklet unavailable",
    );
    expect(audioTrack.stop).toHaveBeenCalledOnce();
  });

  it("stops the preceding stream and resets monitoring when switching devices", async () => {
    const first = microphoneStream();
    const second = microphoneStream();
    getUserMedia
      .mockResolvedValueOnce(first.stream)
      .mockResolvedValueOnce(second.stream);
    await engine.connectMic("first", 0, 4096, vi.fn());
    const firstWorker = FakeWorker.latest;
    engine.setMonitor(true, 1);
    await engine.connectMic("second", 0, 2048, vi.fn());
    expect(first.audioTrack.stop).toHaveBeenCalledOnce();
    expect(firstWorker.terminate).toHaveBeenCalledOnce();
    expect(second.audioTrack.stop).not.toHaveBeenCalled();
    expect(FakeContext.latest.gains[6].gain.value).toBe(0);
    engine.dispose();
    expect(second.audioTrack.stop).toHaveBeenCalledOnce();
  });

  it.each(["disconnect", "dispose"] as const)(
    "cancels a pending permission request after %s",
    async (action) => {
      let grant!: (stream: MediaStream) => void;
      getUserMedia.mockReturnValue(
        new Promise<MediaStream>((resolve) => {
          grant = resolve;
        }),
      );
      const pending = engine.connectMic("", 0, 4096, vi.fn());
      await Promise.resolve();
      expect(getUserMedia).toHaveBeenCalledOnce();
      if (action === "dispose") engine.dispose();
      else engine.disconnectMic();
      const late = microphoneStream();
      grant(late.stream);
      await expect(pending).rejects.toThrow("cancelada");
      expect(late.audioTrack.stop).toHaveBeenCalledOnce();
    },
  );

  it("never lets an older permission result close a newer device stream", async () => {
    let grantOld!: (stream: MediaStream) => void;
    const newer = microphoneStream();
    getUserMedia
      .mockReturnValueOnce(
        new Promise<MediaStream>((resolve) => {
          grantOld = resolve;
        }),
      )
      .mockResolvedValueOnce(newer.stream);
    const oldPending = engine.connectMic("old", 0, 4096, vi.fn());
    await Promise.resolve();
    await engine.connectMic("new", 0, 4096, vi.fn());
    const old = microphoneStream();
    grantOld(old.stream);
    await expect(oldPending).rejects.toThrow("cancelada");
    expect(old.audioTrack.stop).toHaveBeenCalledOnce();
    expect(newer.audioTrack.stop).not.toHaveBeenCalled();
    expect((await engine.getDiagnostics()).deviceLabel).toBe("Test interface");
  });
});
