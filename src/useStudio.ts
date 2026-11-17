import { useCallback, useEffect, useRef, useState } from "react";
import { AudioEngine } from "./audio/engine";
import { emptyStats, PracticeScorer } from "./practice";
import type {
  AudioDiagnostics,
  AudioTrack,
  LiveSample,
  LoopRegion,
  PitchFrame,
  TrackRole,
} from "./types";

export function useStudio() {
  const [engine] = useState(() => new AudioEngine());
  const [tracks, setTracks] = useState<Record<TrackRole, AudioTrack | null>>({
    backing: null,
    target: null,
  });
  const [progress, setProgress] = useState<Record<TrackRole, number | null>>({
    backing: null,
    target: null,
  });
  const [error, setError] = useState("");
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [volumes, setVolumes] = useState({ backing: 0.75, target: 0.65 });
  const [muted, setMuted] = useState({ backing: false, target: false });
  const [loop, setLoopState] = useState<LoopRegion>({
    start: 0,
    end: 10,
    enabled: false,
  });
  const [live, setLive] = useState<LiveSample[]>([]);
  const [currentPitch, setCurrentPitch] = useState<PitchFrame | null>(null);
  const [stats, setStats] = useState(emptyStats);
  const [micActive, setMicActive] = useState(false);
  const [micBusy, setMicBusy] = useState(false);
  const [devices, setDevices] = useState<MediaDeviceInfo[]>([]);
  const [deviceId, setDeviceId] = useState("");
  const [channel, setChannel] = useState(0);
  const [windowSize, setWindowSize] = useState(4096);
  const [monitor, setMonitorState] = useState(false);
  const [inputGain, setInputGainState] = useState(1);
  const [diagnostics, setDiagnostics] = useState<AudioDiagnostics | null>(null);
  const [latency, setLatencyState] = useState(0);
  const [tolerance, setToleranceState] = useState(50);
  const scorer = useRef(new PracticeScorer());
  const disposalTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const state = useRef({ tracks, latency, tolerance, micActive });
  state.current = { tracks, latency, tolerance, micActive };
  const scoringArmed = useRef(false);
  const duration = Math.max(
    tracks.backing?.buffer.duration ?? 0,
    tracks.target?.buffer.duration ?? 0,
  );
  const resetAttempt = useCallback(() => {
    scorer.current.reset();
    scoringArmed.current = false;
    setStats(emptyStats);
    setLive([]);
  }, []);
  const refreshDevices = useCallback(async () => {
    if (!navigator.mediaDevices?.enumerateDevices) return;
    setDevices(await navigator.mediaDevices.enumerateDevices());
  }, []);
  useEffect(() => {
    if (disposalTimer.current) clearTimeout(disposalTimer.current);
    void refreshDevices().catch(() => {});
    navigator.mediaDevices?.addEventListener("devicechange", refreshDevices);
    return () => {
      navigator.mediaDevices?.removeEventListener(
        "devicechange",
        refreshDevices,
      );
      // React may immediately set effects up again during development refresh.
      disposalTimer.current = setTimeout(() => engine.dispose(), 0);
    };
  }, [engine, refreshDevices]);
  useEffect(() => {
    engine.onEnded = () => setPlaying(false);
    engine.onLoop = resetAttempt;
    let raf = 0,
      last = 0;
    const update = (stamp: number) => {
      if (stamp - last > 30) {
        const position = engine.getPosition();
        setTime(position);
        setPlaying(engine.isPlaying);
        if (
          engine.isPlaying &&
          (state.current.micActive || scoringArmed.current) &&
          state.current.tracks.target
        ) {
          scoringArmed.current = true;
          setStats(
            scorer.current.advance(state.current.tracks.target.pitch, position),
          );
        }
        last = stamp;
      }
      raf = requestAnimationFrame(update);
    };
    raf = requestAnimationFrame(update);
    return () => cancelAnimationFrame(raf);
  }, [engine, resetAttempt]);
  const run = useCallback(async (action: () => Promise<void>) => {
    try {
      setError("");
      await action();
    } catch (e) {
      setError(
        e instanceof Error
          ? e.message
          : "Não foi possível concluir a operação.",
      );
    }
  }, []);
  const loadFile = (role: TrackRole, file: File) =>
    run(async () => {
      setProgress((p) => ({ ...p, [role]: 0 }));
      try {
        const track = await engine.loadFile(file, role, (n) =>
          setProgress((p) => ({ ...p, [role]: n })),
        );
        engine.setTrack(role, track);
        engine.setVolume(role, muted[role] ? 0 : volumes[role]);
        setTracks((t) => ({ ...t, [role]: track }));
        const freshLoop = {
          start: 0,
          end: Math.min(10, engine.duration),
          enabled: false,
        };
        setLoopState(freshLoop);
        engine.setLoop(freshLoop);
        engine.seek(0);
        resetAttempt();
        setPlaying(false);
      } finally {
        setProgress((p) => ({ ...p, [role]: null }));
      }
    });
  const demo = () =>
    run(async () => {
      setProgress({ backing: 0, target: 0 });
      try {
        const next = await engine.loadDemo((n) =>
          setProgress({ backing: n, target: n }),
        );
        for (const role of ["backing", "target"] as const) {
          engine.setTrack(role, next[role]);
          engine.setVolume(role, muted[role] ? 0 : volumes[role]);
        }
        setTracks(next);
        const demoLoop = {
          start: 0,
          end: Math.min(10, next.target.buffer.duration),
          enabled: false,
        };
        setLoopState(demoLoop);
        engine.setLoop(demoLoop);
        engine.seek(0);
        resetAttempt();
        setPlaying(false);
      } finally {
        setProgress({ backing: null, target: null });
      }
    });
  const togglePlayback = useCallback(
    () =>
      run(async () => {
        if (engine.isPlaying) {
          engine.pause();
          setPlaying(false);
        } else {
          await engine.play(engine.getPosition());
          setPlaying(true);
        }
      }),
    [engine, run],
  );
  const seek = useCallback(
    (seconds: number) => {
      engine.seek(Math.max(0, Math.min(duration, seconds)));
      setTime(engine.getPosition());
      resetAttempt();
    },
    [duration, engine, resetAttempt],
  );
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const element = e.target as HTMLElement;
      if (
        element.isContentEditable ||
        /INPUT|SELECT|TEXTAREA|BUTTON/.test(element.tagName) ||
        e.ctrlKey ||
        e.metaKey ||
        e.altKey
      )
        return;
      if (e.code === "Space" && duration > 0) {
        e.preventDefault();
        if (!e.repeat) void togglePlayback();
      }
      if (e.code === "Enter" && duration > 0) {
        e.preventDefault();
        seek(0);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [duration, togglePlayback, seek]);
  const setVolume = (role: TrackRole, value: number) => {
    setVolumes((v) => ({ ...v, [role]: value }));
    engine.setVolume(role, muted[role] ? 0 : value);
  };
  const toggleMute = (role: TrackRole) => {
    setMuted((v) => ({ ...v, [role]: !v[role] }));
    engine.setVolume(role, muted[role] ? volumes[role] : 0);
  };
  const setLoop = (next: LoopRegion) => {
    const safe = {
      ...next,
      start: Math.max(0, Math.min(next.start, duration - 0.1)),
      end: Math.max(0.1, Math.min(next.end, duration)),
    };
    if (safe.end <= safe.start) safe.end = Math.min(duration, safe.start + 0.1);
    setLoopState(safe);
    engine.setLoop(safe);
  };
  const onPitch = useCallback(
    (p: PitchFrame) => {
      setCurrentPitch(p);
      if (!engine.isPlaying) return;
      const s = state.current;
      const aligned = p;
      if (aligned.time < 0) return;
      setLive((points) =>
        [
          ...points.filter(
            (v) => v.time > aligned.time - 90 && v.time <= aligned.time,
          ),
          aligned,
        ].slice(-4500),
      );
      if (s.tracks.target)
        setStats(
          scorer.current.sample(
            s.tracks.target.pitch,
            aligned.time,
            p,
            s.tolerance,
          ),
        );
    },
    [engine],
  );
  const disconnectMic = () => {
    engine.disconnectMic();
    setMicBusy(false);
    setMicActive(false);
    setMonitorState(false);
    setCurrentPitch(null);
  };
  const connectMic = (id = deviceId, ch = channel, win = windowSize) =>
    run(async () => {
      setMicBusy(true);
      setMonitorState(false);
      setMicActive(false);
      try {
        const d = await engine.connectMic(id, ch, win, onPitch, () => {
          setMicActive(false);
          setMonitorState(false);
          setCurrentPitch(null);
        });
        setDiagnostics(d);
        setMicActive(true);
        engine.setInputGain(inputGain);
        engine.setMonitor(false, 1);
        await refreshDevices();
      } finally {
        setMicBusy(false);
      }
    });
  const changeDevice = (id: string) => {
    setDeviceId(id);
    setChannel(0);
    if (micActive) void connectMic(id, 0);
  };
  const changeChannel = (ch: number) => {
    setChannel(ch);
    if (micActive) void connectMic(deviceId, ch);
  };
  const changeWindow = (win: number) => {
    setWindowSize(win);
    if (micActive) void connectMic(deviceId, channel, win);
  };
  const setMonitor = (value: boolean) => {
    setMonitorState(value);
    engine.setMonitor(value, 1);
  };
  const setInputGain = (value: number) => {
    setInputGainState(value);
    engine.setInputGain(value);
  };
  const setLatency = (value: number) => {
    setLatencyState(value);
    engine.setLatencyCompensation(value);
    resetAttempt();
  };
  const setTolerance = (value: number) => {
    setToleranceState(value);
    resetAttempt();
  };
  const setOutput = (id: string) => run(() => engine.setOutputDevice(id));
  return {
    tracks,
    progress,
    error,
    setError,
    playing,
    time,
    duration,
    volumes,
    muted,
    loop,
    live,
    currentPitch,
    stats,
    micActive,
    micBusy,
    devices,
    deviceId,
    channel,
    windowSize,
    monitor,
    inputGain,
    diagnostics,
    latency,
    tolerance,
    loadFile,
    demo,
    togglePlayback,
    seek,
    setVolume,
    toggleMute,
    setLoop,
    resetAttempt,
    connectMic,
    disconnectMic,
    changeDevice,
    changeChannel,
    changeWindow,
    setMonitor,
    setInputGain,
    setLatency,
    setTolerance,
    setOutput,
  };
}
