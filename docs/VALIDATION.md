# Native validation — v0.5

Validated on Apple Silicon macOS with Rust 1.100.0, GPUI 0.2.2, CPAL 0.18.2 and Symphonia 0.5.5. The optimized application bundle builds and passes ad hoc signature verification.

## Automated evidence

- Formatting, all-target compilation, warning-free Clippy and release build pass.
- Core suite: **86 passed, 4 opt-in corpus/hardware probes ignored**. Desktop suite: **4 passed**, covering permission completion/cancellation and recorded-result association with the original reference and polyphonic line.
- Recording tests exercise selected-channel PCM, arming before playback, capture independent of monitor gain, bounded backpressure, explicit overflow/disk/input failures, stop/finalization, PCM32 RIFF/RF64 headers and malformed-container rejection. Replay tests cover bounded decoding, resampling, natural EOF, cancellation and preserving the main transport position. Faders bypass a full hardware-owner queue.
- Persistence tests cover exact original/recording bytes, version-1 migration, version-2 metadata, extraction lifetime, missing/mismatched recordings, ZIP64, unsafe paths, symlinks, CRC, truncated archives, Unicode and failed writes preserving the old destination. Preferences tests use disposable paths and cover remembered routing, defaults, malformed/future files and monitoring forced off.
- Existing bounded-media evidence remains: a 901-second / 86 MB sparse WAV round-trips through `.voice`; an approximately 800 MB, over-one-hour sparse WAV is inspected, sought and read in bounded blocks. These are storage/decoder checks, not an hour-long microphone recording benchmark.
- Document lifecycle tests cover Save/Don't Save/Cancel, asynchronous save completion, cancellation, disk failure and edits during a save. Native quit interception forwards through the same document lifecycle; recording finalization completes before saving or replacing a document.

## Actual native runtime checks

The v0.5 preview used a version-specific bundle identifier, a separate preferences file through `VOICE_PREFERENCES_PATH`, and generated test media under the external task-output directory. The user's running older Voice session was not restarted, replaced or reconfigured.

- The initial screen staged both `target.wav` and `backing.wav`, with the reference-content choice visible. Continue imported/analyzed both files without a duplicate content question.
- The persistent Mixer, device dropdowns, cached mono-channel choices and three faders rendered in the system dark appearance. Scarlett channels 1 and 2 were offered before enabling the microphone. Pointer dragging set the voice fader to silence; reference/accompaniment mute controls worked. The layout was inspected at the default size and approximately 1060 × 680 content size; the toolbar wrapped and the mixer remained scrollable. Light tokens compile but were not toggled in this runtime pass.
- Choosing Scarlett input and channel 2 saved those preferences. Selecting an explicit Scarlett output automatically restored the active input/monitoring state after reconfiguration. Restarting the preview restored input, output and channel 2, with the microphone and monitoring off.
- **Record take** connected the microphone and started the reference transport in one action. Two approximately 12-second attempts were captured on the selected mono channel, finalized and exposed through the sidebar and Practice History. This was a signal-path test; no sung voice was detected on that channel. The resulting 0% coverage / 100% missed result is evidence of handling absent voiced input, not singing accuracy.
- **Hear myself** from microphone-off enabled capture and software monitoring together. The voice fader remained at silence during this check to avoid audible feedback. This verifies state/routing, not acoustic monitor quality or physical latency.
- **Listen to take** decoded saved audio, advanced the replay cursor, displayed the stored result and kept the main song transport separate. The final transport uses Stop listening/Space to stop an audition. A restarted application reopened the packaged takes and offered their audio again.
- Native Save As wrote `Mixer recordings.voice`. Independent ZIP/JSON/WAV reads confirmed format v2, both source files byte-identical, two mono PCM32 recordings at 44,100 Hz with 529,236 and 529,192 frames, stored traces/results and approximately 11.88 evaluated seconds per take. Input/output/channel preferences survived independently of the project. Test recordings remain outside Git.
- Saving and quitting exited normally. Existing native file sheets, represented-file/edited chrome and Save/Don't Save/Cancel protection remain in place. The external Dock quit entry point has source/build evidence from v0.4; its Dock UI surface was unavailable in that pass.

## Analysis evidence

Pitch analysis algorithms were not changed by v0.5, so the full corpus benchmark was not rerun. The separately documented native 40-clip Vocadito run measured RPA50c 84.5578%, RCA 85.6926%, voicing recall 91.5657% and false alarm 10.4191%; see [NATIVE_BENCHMARK.md](NATIVE_BENCHMARK.md). Historical JavaScript/polyphonic results remain under research/ and do not establish native accuracy. Native model inference/chunking already has synthetic and real-recording smoke evidence.

## Remaining limits

Polyphonic candidates do not identify singers or choir sections; the selected line requires review. A stored result is not recomputed under a changed tolerance/reference line, and comparison playback is guarded against a replaced reference or a different scored polyphonic selection. Live pitch expects one voice. Sample-rate conversion uses linear interpolation. Capture buffers are bounded; an overwhelmed recorder reports failure rather than silently dropping audio. Analysis/history metadata and disk needs grow with duration. Saves need temporary space for a second archive; opening extracts assets to a private workspace. Actual sung performance, audible monitoring and physical round-trip latency still require user/hardware evaluation. Bundles are locally ad hoc signed; notarized distribution and Intel Mac validation are outside this local build.
