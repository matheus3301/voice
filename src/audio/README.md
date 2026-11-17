# Audio engine contract

The engine owns one lazily-created AudioContext. Both tracks use its sample clock and the same source start time. Source scheduling is ahead of that clock; the 25 ms JS timer only schedules and reports lifecycle changes. Loops create a new pair of sources at each boundary so a shorter track remains silent instead of repeating early. A delayed background timer recovers at the current transport position rather than stacking missed sources.

`loadFile` returns an AudioTrack without installing it; call `setTrack` only after a successful result. Progress is 0…1. Inputs are limited to 80 MiB and 15 minutes per track. Audio decoding is browser-native; unsupported formats report an error. Analysis runs in a module Worker and uses downsampled 16 kHz mono, YIN, RMS/confidence gates, 25 ms hop and note segmentation. Backing tracks are only analyzed for waveform. Multichannel files use the first two channels for display/target analysis and preserve all channels for playback.

`PitchFrame.time` from microphone capture is the **transport time at the center of the captured window**, mapped from AudioWorklet sample time. `setLatencyCompensation(milliseconds)` subtracts manual compensation on the AudioContext capture clock **before** transport/loop mapping. The UI must use frame.time directly and must not subtract compensation or the analysis-window delay again. Hardware input/output latency is not measured automatically. Results arriving after seek, pause, compensation change or a loop iteration change are discarded. Capture windows predating a new playback start are discarded even if their port messages arrive later. While paused the callback still provides pitch/RMS at the current cursor for a live tuner.

Unvoiced frames use `frequency: 0`, `midi: NaN`, `confidence: 0`, with RMS retained for metering. Offline and demo pitch arrays include silence frames every 25 ms. Never interpolate target notes across these silence frames. MIDI values are fractional (A4=69, 440 Hz).

`connectMic` always starts with monitoring off, closes the preceding stream, requests unprocessed input and selects a channel exposed by that stream. Changing devices can invalidate channel choice. `getDiagnostics` reports the AudioContext sample rate, base/output latency in **seconds**, stream channel count and device label. A browser can omit/ignore hardware properties. Window sizes (2048/4096/8192 samples) change analysis responsiveness and low-note reliability; they are not driver buffers. `setOutputDevice` rejects when AudioContext.setSinkId is unsupported.

`dispose` permanently releases the instance. React StrictMode's effect setup/cleanup simulation must not permanently dispose a retained instance on its initial mount.

Demo audio is synthesized from sine/harmonic tones and accompaniment, without external audio assets. Its target is a 24 s C-major study with explicit gaps and ground-truth pitch.
