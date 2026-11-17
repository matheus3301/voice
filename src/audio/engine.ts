/// <reference types="vite/client" />
import type {
  AudioDiagnostics,
  AudioTrack,
  LoopRegion,
  PitchFrame,
  TrackRole,
} from "../types";
import { midiToFrequency, createWaveform, segmentNotes } from "./pitch";
import { capturePosition, transportPosition, validLoop } from "./transport";

interface AnalysisResult {
  waveform: Float32Array;
  pitch: PitchFrame[];
  notes: AudioTrack["notes"];
}
interface PendingAnalysis {
  resolve: (result: AnalysisResult) => void;
  reject: (error: Error) => void;
  progress?: (value: number) => void;
}
interface SinkAudioContext extends AudioContext {
  setSinkId: (deviceId: string) => Promise<void>;
}
const MAX_FILE_SIZE = 80 * 1024 * 1024;
const MAX_DURATION = 15 * 60;

/** Local-only Web Audio engine. React owns user settings, this class owns all audio resources. */
export class AudioEngine {
  private context: AudioContext | null = null;
  private gains: Partial<Record<TrackRole, GainNode>> = {};
  private tracks: Partial<Record<TrackRole, AudioTrack>> = {};
  private volumes: Record<TrackRole, number> = { backing: 0.75, target: 0.65 };
  private activeSources = new Set<AudioBufferSourceNode>();
  private playing = false;
  private offset = 0;
  private startedAt = 0;
  private scheduledUntil = 0;
  private nextSegmentOffset = 0;
  private transportGeneration = 0;
  private timer: ReturnType<typeof setInterval> | null = null;
  private loop: LoopRegion = { start: 0, end: 0, enabled: false };
  private loopIteration = 0;
  private analysisWorker: Worker | null = null;
  private pendingAnalysis = new Map<number, PendingAnalysis>();
  private analysisId = 0;
  private stream: MediaStream | null = null;
  private micSource: MediaStreamAudioSourceNode | null = null;
  private splitter: ChannelSplitterNode | null = null;
  private inputGain: GainNode | null = null;
  private monitorGain: GainNode | null = null;
  private capture: AudioWorkletNode | null = null;
  private liveWorker: Worker | null = null;
  private silentGain: GainNode | null = null;
  private micGeneration = 0;
  private inputGainValue = 1;
  private latencyCompensation = 0;
  private workletLoaded = false;
  private liveBusy = false;
  private disposed = false;
  onEnded: () => void = () => {};
  onLoop: () => void = () => {};

  get isPlaying(): boolean {
    return this.playing;
  }
  get duration(): number {
    return Math.max(
      this.tracks.backing?.buffer.duration ?? 0,
      this.tracks.target?.buffer.duration ?? 0,
    );
  }

  private audioContext(): AudioContext {
    if (this.disposed) throw new Error("A sessão de áudio foi encerrada.");
    if (!this.context) {
      this.context = new AudioContext({ latencyHint: "interactive" });
      for (const role of ["backing", "target"] as const) {
        const gain = this.context.createGain();
        gain.gain.value = this.volumes[role];
        gain.connect(this.context.destination);
        this.gains[role] = gain;
      }
    }
    return this.context;
  }

  private analyzer(): Worker {
    if (!this.analysisWorker) {
      this.analysisWorker = new Worker(
        new URL("./analysis.worker.ts", import.meta.url),
        { type: "module" },
      );
      this.analysisWorker.onmessage = (
        event: MessageEvent<{
          id: number;
          progress?: number;
          result?: AnalysisResult;
          error?: string;
        }>,
      ) => {
        const pending = this.pendingAnalysis.get(event.data.id);
        if (!pending) return;
        if (event.data.progress !== undefined)
          pending.progress?.(event.data.progress);
        if (event.data.result) {
          this.pendingAnalysis.delete(event.data.id);
          pending.resolve(event.data.result);
        } else if (event.data.error) {
          this.pendingAnalysis.delete(event.data.id);
          pending.reject(new Error(event.data.error));
        }
      };
      this.analysisWorker.onerror = () => {
        for (const pending of this.pendingAnalysis.values())
          pending.reject(
            new Error(
              "Não foi possível analisar o arquivo. Tente outro formato.",
            ),
          );
        this.pendingAnalysis.clear();
        this.analysisWorker?.terminate();
        this.analysisWorker = null;
      };
    }
    return this.analysisWorker;
  }

  async loadFile(
    file: File,
    role: TrackRole,
    onProgress?: (value: number) => void,
  ): Promise<AudioTrack> {
    if (file.size > MAX_FILE_SIZE)
      throw new Error("O limite é 80 MB por arquivo. Exporte uma faixa menor.");
    if (file.size === 0) throw new Error("Esse arquivo está vazio.");
    const context = this.audioContext();
    onProgress?.(0.02);
    let buffer: AudioBuffer;
    try {
      buffer = await context.decodeAudioData(await file.arrayBuffer());
    } catch {
      throw new Error(
        "Não foi possível abrir esse áudio. Tente WAV, MP3, M4A, OGG ou FLAC compatível com seu navegador.",
      );
    }
    if (buffer.duration > MAX_DURATION)
      throw new Error(
        "O limite da v0 é 15 minutos por faixa. Exporte um trecho menor.",
      );
    if (buffer.duration < 0.1 || !buffer.numberOfChannels)
      throw new Error("O áudio precisa ter pelo menos 0,1 segundo.");
    onProgress?.(0.08);
    const channels: Float32Array[] = [];
    // Copy: transferring the analysis input must never detach playback's AudioBuffer.
    for (let i = 0; i < Math.min(buffer.numberOfChannels, 2); i++)
      channels.push(buffer.getChannelData(i).slice());
    const result = await new Promise<AnalysisResult>((resolve, reject) => {
      const id = ++this.analysisId;
      this.pendingAnalysis.set(id, {
        resolve,
        reject,
        progress: (value) => onProgress?.(0.08 + value * 0.92),
      });
      this.analyzer().postMessage(
        {
          id,
          channels,
          sampleRate: buffer.sampleRate,
          duration: buffer.duration,
          analyze: role === "target",
        },
        channels.map((channel) => channel.buffer),
      );
    });
    onProgress?.(1);
    return { name: file.name, buffer, ...result };
  }

  setTrack(role: TrackRole, track: AudioTrack): void {
    this.pause();
    this.tracks[role] = track;
    this.offset = Math.min(this.offset, this.duration);
    this.loop = validLoop(this.loop, this.duration);
  }

  setVolume(role: TrackRole, volume: number): void {
    this.volumes[role] = Math.max(0, Math.min(1, volume));
    const context = this.context;
    if (context && this.gains[role])
      this.gains[role]!.gain.setTargetAtTime(
        this.volumes[role],
        context.currentTime,
        0.01,
      );
  }

  async play(offset: number): Promise<void> {
    const context = this.audioContext();
    this.pause();
    if (!this.duration) return;
    const generation = ++this.transportGeneration;
    await context.resume();
    if (generation !== this.transportGeneration || this.disposed) return;
    this.offset = Math.max(0, Math.min(this.duration, offset));
    if (this.offset >= this.duration) this.offset = 0;
    if (this.loop.enabled && this.offset >= this.loop.end)
      this.offset = this.loop.start;
    this.startedAt = context.currentTime + 0.02;
    this.scheduledUntil = this.startedAt;
    this.nextSegmentOffset = this.offset;
    this.loopIteration = 0;
    this.playing = true;
    this.schedule();
    this.timer = setInterval(() => this.schedule(), 25);
  }

  private schedule(): void {
    const context = this.context;
    if (!context || !this.playing) return;
    const elapsed = Math.max(0, context.currentTime - this.startedAt);
    const raw = this.offset + elapsed;
    if (!this.loop.enabled && raw >= this.duration) {
      this.pause();
      this.offset = this.duration;
      this.onEnded();
      return;
    }
    if (this.loop.enabled && raw >= this.loop.end) {
      const iteration =
        1 +
        Math.floor((raw - this.loop.end) / (this.loop.end - this.loop.start));
      if (iteration > this.loopIteration) {
        this.loopIteration = iteration;
        this.onLoop();
      }
    }
    // Recover from a background-tab timer stall without starting missed loop sources together.
    if (this.scheduledUntil < context.currentTime - 0.05) {
      this.scheduledUntil = context.currentTime + 0.02;
      this.nextSegmentOffset = this.positionAt(this.scheduledUntil);
    }
    // Schedule ahead on the audio clock; timer jitter never controls actual source start times.
    while (this.scheduledUntil < context.currentTime + 0.15) {
      const end = this.loop.enabled ? this.loop.end : this.duration;
      const segmentDuration = end - this.nextSegmentOffset;
      if (segmentDuration <= 0) break;
      for (const role of ["backing", "target"] as const) {
        const track = this.tracks[role];
        if (!track || this.nextSegmentOffset >= track.buffer.duration) continue;
        const source = context.createBufferSource();
        source.buffer = track.buffer;
        source.connect(this.gains[role]!);
        this.activeSources.add(source);
        source.onended = () => {
          source.disconnect();
          this.activeSources.delete(source);
        };
        source.start(
          this.scheduledUntil,
          this.nextSegmentOffset,
          Math.min(
            segmentDuration,
            track.buffer.duration - this.nextSegmentOffset,
          ),
        );
      }
      this.scheduledUntil += segmentDuration;
      if (!this.loop.enabled) break;
      this.nextSegmentOffset = this.loop.start;
    }
  }

  getPosition(): number {
    return this.positionAt(this.context?.currentTime ?? 0);
  }

  private positionAt(audioTime: number): number {
    if (!this.playing) return this.offset;
    return transportPosition(
      this.offset,
      Math.max(0, audioTime - this.startedAt),
      this.duration,
      this.loop,
    );
  }

  pause(): number {
    const position = this.getPosition();
    this.playing = false;
    this.offset = position;
    this.loopIteration = 0;
    this.transportGeneration++;
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    for (const source of this.activeSources) {
      try {
        source.stop();
      } catch {
        /* already ended */
      }
      source.disconnect();
    }
    this.activeSources.clear();
    return position;
  }

  seek(offset: number): void {
    const wasPlaying = this.playing;
    this.pause();
    this.offset = Math.max(0, Math.min(this.duration, offset));
    if (wasPlaying) void this.play(this.offset);
  }

  setLoop(loop: LoopRegion): void {
    const wasPlaying = this.playing;
    const position = this.pause();
    this.loop = validLoop(loop, this.duration);
    if (wasPlaying) void this.play(position);
  }

  async connectMic(
    deviceId: string,
    channel: number,
    windowSize: number,
    onPitch: (frame: PitchFrame) => void,
    onEnded?: () => void,
  ): Promise<AudioDiagnostics> {
    this.disconnectMic();
    const generation = this.micGeneration;
    const context = this.audioContext();
    if (!navigator.mediaDevices?.getUserMedia)
      throw new Error(
        "Microfone indisponível. Abra em HTTPS ou localhost e use um navegador compatível.",
      );
    await context.resume();
    let stream: MediaStream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          ...(deviceId ? { deviceId: { exact: deviceId } } : {}),
          channelCount: { ideal: 2 },
          echoCancellation: false,
          noiseSuppression: false,
          autoGainControl: false,
        },
      });
    } catch (error) {
      const name = error instanceof DOMException ? error.name : "";
      if (name === "NotAllowedError")
        throw new Error(
          "Permita o acesso ao microfone nas configurações do navegador.",
        );
      if (name === "NotFoundError" || name === "OverconstrainedError")
        throw new Error(
          "Essa entrada não está disponível. Conecte a interface e atualize a lista.",
        );
      throw new Error(
        "Não foi possível abrir a entrada. Verifique se a interface está conectada e disponível.",
      );
    }
    if (generation !== this.micGeneration || this.disposed) {
      stream.getTracks().forEach((track) => track.stop());
      throw new Error("A abertura do microfone foi cancelada.");
    }
    this.stream = stream;
    try {
      const settings = stream.getAudioTracks()[0].getSettings();
      const channelCount = Math.max(1, settings.channelCount ?? 1);
      if (channel >= channelCount || channel < 0)
        throw new Error(
          `A entrada oferece ${channelCount} canal(is) ao navegador. Escolha um canal disponível.`,
        );
      if (!this.workletLoaded) {
        await context.audioWorklet.addModule(
          `${import.meta.env.BASE_URL}pitch-capture.js`,
        );
        this.workletLoaded = true;
      }
      if (generation !== this.micGeneration || this.disposed)
        throw new Error("A abertura do microfone foi cancelada.");
      this.micSource = context.createMediaStreamSource(stream);
      this.splitter = context.createChannelSplitter(channelCount);
      this.inputGain = context.createGain();
      this.inputGain.gain.value = this.inputGainValue;
      this.monitorGain = context.createGain();
      this.monitorGain.gain.value = 0;
      const size = [2048, 4096, 8192].includes(windowSize) ? windowSize : 4096;
      this.capture = new AudioWorkletNode(context, "pitch-capture", {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        outputChannelCount: [1],
        channelCount: 1,
        processorOptions: { windowSize: size },
      });
      this.silentGain = context.createGain();
      this.silentGain.gain.value = 0;
      this.micSource.connect(this.splitter);
      this.splitter.connect(this.inputGain, channel, 0);
      this.inputGain.connect(this.capture);
      this.inputGain.connect(this.monitorGain);
      this.monitorGain.connect(context.destination);
      this.capture.connect(this.silentGain);
      this.silentGain.connect(context.destination);
      this.liveWorker = new Worker(
        new URL("./live.worker.ts", import.meta.url),
        { type: "module" },
      );
      this.liveWorker.onmessage = (
        event: MessageEvent<{
          frame: PitchFrame;
          generation: number;
          transportGeneration: number;
          loopIteration: number;
        }>,
      ) => {
        this.liveBusy = false;
        if (
          event.data.generation === this.micGeneration &&
          event.data.transportGeneration === this.transportGeneration &&
          event.data.loopIteration === this.loopIteration
        )
          onPitch(event.data.frame);
      };
      this.liveWorker.onerror = () => {
        this.disconnectMic();
        onEnded?.();
      };
      this.capture.port.onmessage = (
        event: MessageEvent<{
          samples: Float32Array;
          sampleRate: number;
          centerTime: number;
        }>,
      ) => {
        if (
          generation !== this.micGeneration ||
          this.liveBusy ||
          !this.liveWorker
        )
          return;
        const { samples, sampleRate, centerTime } = event.data;
        const position = capturePosition(
          centerTime,
          this.latencyCompensation,
          this.startedAt,
          this.playing,
          this.offset,
          this.duration,
          this.loop,
        );
        if (!position) return;
        this.liveBusy = true;
        this.liveWorker.postMessage(
          {
            samples,
            sampleRate,
            time: position.time,
            generation,
            transportGeneration: this.transportGeneration,
            loopIteration: position.iteration,
          },
          [samples.buffer],
        );
      };
      stream.getAudioTracks()[0].onended = () => {
        if (generation === this.micGeneration) {
          this.disconnectMic();
          onEnded?.();
        }
      };
      return this.getDiagnostics();
    } catch (error) {
      if (generation === this.micGeneration) this.disconnectMic();
      else stream.getTracks().forEach((track) => track.stop());
      throw error;
    }
  }

  disconnectMic(): void {
    this.micGeneration++;
    this.liveBusy = false;
    if (this.capture) this.capture.port.onmessage = null;
    for (const node of [
      this.micSource,
      this.splitter,
      this.inputGain,
      this.monitorGain,
      this.capture,
      this.silentGain,
    ])
      node?.disconnect();
    this.micSource = null;
    this.splitter = null;
    this.inputGain = null;
    this.monitorGain = null;
    this.capture = null;
    this.silentGain = null;
    this.liveWorker?.terminate();
    this.liveWorker = null;
    this.stream?.getTracks().forEach((track) => {
      track.onended = null;
      track.stop();
    });
    this.stream = null;
  }

  setMonitor(enabled: boolean, gain: number): void {
    if (this.context && this.monitorGain)
      this.monitorGain.gain.setTargetAtTime(
        enabled ? Math.max(0, Math.min(1, gain)) : 0,
        this.context.currentTime,
        0.01,
      );
  }

  setInputGain(gain: number): void {
    this.inputGainValue = Math.max(0, Math.min(4, gain));
    if (this.context && this.inputGain)
      this.inputGain.gain.setTargetAtTime(
        this.inputGainValue,
        this.context.currentTime,
        0.01,
      );
  }

  setLatencyCompensation(milliseconds: number): void {
    this.latencyCompensation = Number.isFinite(milliseconds)
      ? Math.max(-2000, Math.min(2000, milliseconds)) / 1000
      : 0;
    this.transportGeneration++;
  }

  async getDiagnostics(): Promise<AudioDiagnostics> {
    const context = this.audioContext();
    const track = this.stream?.getAudioTracks()[0];
    return {
      sampleRate: context.sampleRate,
      baseLatency: context.baseLatency ?? 0,
      outputLatency: context.outputLatency ?? 0,
      channels: track?.getSettings().channelCount ?? 1,
      deviceLabel: track?.label ?? "Entrada não ativada",
    };
  }

  async setOutputDevice(deviceId: string): Promise<void> {
    const context = this.audioContext();
    if (!("setSinkId" in context))
      throw new Error(
        "Este navegador usa a saída de áudio selecionada no sistema.",
      );
    await (context as SinkAudioContext).setSinkId(deviceId);
  }

  async loadDemo(
    onProgress?: (value: number) => void,
  ): Promise<{ backing: AudioTrack; target: AudioTrack }> {
    const context = this.audioContext();
    const sampleRate = context.sampleRate;
    const duration = 24;
    const backingBuffer = context.createBuffer(
      2,
      duration * sampleRate,
      sampleRate,
    );
    const targetBuffer = context.createBuffer(
      1,
      duration * sampleRate,
      sampleRate,
    );
    const melody = [
      60, 62, 64, 67, 64, 62, 60, 60, 64, 65, 67, 69, 67, 65, 64, 62,
    ];
    const notes: AudioTrack["notes"] = [];
    const pitch: PitchFrame[] = [];
    for (let index = 0; index < melody.length; index++) {
      const midi = melody[index];
      const start = 1 + index * 1.35;
      const end = start + 1.08;
      const frequency = midiToFrequency(midi);
      notes.push({ start, end, midi });
      const channel = targetBuffer.getChannelData(0);
      for (
        let i = Math.floor(start * sampleRate);
        i < Math.floor(end * sampleRate);
        i++
      ) {
        const t = i / sampleRate - start;
        const envelope = Math.min(1, t / 0.025, (end - i / sampleRate) / 0.06);
        const phase = 2 * Math.PI * frequency * t;
        channel[i] =
          envelope * (0.19 * Math.sin(phase) + 0.035 * Math.sin(phase * 2));
      }
    }
    for (let frame = 0; frame < duration / 0.025; frame++) {
      const time = frame * 0.025;
      const note = notes.find((note) => time >= note.start && time < note.end);
      pitch.push(
        note
          ? {
              time,
              frequency: midiToFrequency(note.midi),
              midi: note.midi,
              confidence: 1,
              rms: 0.14,
            }
          : { time, frequency: 0, midi: Number.NaN, confidence: 0, rms: 0 },
      );
    }
    const chords = [
      [48, 52, 55],
      [45, 48, 52],
      [41, 45, 48],
      [43, 47, 50],
    ];
    for (let i = 0; i < backingBuffer.length; i++) {
      const t = i / sampleRate;
      const chord = chords[Math.floor(t / 6) % chords.length];
      const beat = t % 0.75;
      const envelope = Math.min(1, t / 0.1, (duration - t) / 0.3);
      let value = 0;
      for (const midi of chord)
        value += Math.sin(2 * Math.PI * midiToFrequency(midi) * t) * 0.035;
      value += Math.sin(2 * Math.PI * 70 * beat) * Math.exp(-beat * 28) * 0.12;
      backingBuffer.getChannelData(0)[i] = value * envelope;
      backingBuffer.getChannelData(1)[i] = value * envelope;
    }
    const backing = {
      name: "Demonstração · acompanhamento",
      buffer: backingBuffer,
      waveform: createWaveform([backingBuffer.getChannelData(0)]),
      pitch: [],
      notes: [],
    };
    const target = {
      name: "Demonstração · melodia em Dó maior",
      buffer: targetBuffer,
      waveform: createWaveform([targetBuffer.getChannelData(0)]),
      pitch,
      notes: segmentNotes(pitch, duration),
    };
    onProgress?.(1);
    return { backing, target };
  }

  dispose(): void {
    this.pause();
    this.disconnectMic();
    this.analysisWorker?.terminate();
    this.analysisWorker = null;
    for (const pending of this.pendingAnalysis.values())
      pending.reject(new Error("A sessão foi encerrada."));
    this.pendingAnalysis.clear();
    if (this.context) void this.context.close();
    this.context = null;
    this.tracks = {};
    this.disposed = true;
  }
}
