export type TrackRole = "backing" | "target";
export interface PitchFrame {
  time: number;
  frequency: number;
  midi: number;
  confidence: number;
  rms: number;
}
export interface NoteSegment {
  start: number;
  end: number;
  midi: number;
}
export interface AudioTrack {
  name: string;
  buffer: AudioBuffer;
  waveform: Float32Array;
  pitch: PitchFrame[];
  notes: NoteSegment[];
}
export interface LiveSample extends PitchFrame {}
export interface LoopRegion {
  start: number;
  end: number;
  enabled: boolean;
}
export interface AudioDiagnostics {
  sampleRate: number;
  baseLatency: number;
  outputLatency: number;
  channels: number;
  deviceLabel: string;
}
export interface PracticeStats {
  score: number;
  coverage: number;
  cents: number | null;
  targetMidi: number | null;
  matchedSeconds: number;
  targetSeconds: number;
}
