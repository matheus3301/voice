# Native validation

Validated on Apple Silicon macOS with Rust 1.100.0, GPUI 0.2.2, CPAL 0.18.2 and Symphonia 0.5.5. The release binary builds successfully and the local application bundle is ad hoc signed.

## Automated evidence

- `cargo fmt --all --check`, `cargo check --all-targets`, warning-free Clippy and optimized native build pass.
- Core unit/integration suite: 48 passed; 4 opt-in corpus/hardware tests ignored by the default suite. The corpus benchmark is run separately against the local authorized Vocadito cache.
- Real bundled ONNX model inference runs natively, including simultaneous-note synthetic input and multi-chunk real singing.
- Project tests cover original-byte round trips, cached analysis, settings/history, future versions, unsafe paths, symlinks, corrupt/truncated files, CRC, ZIP64 headers, Unicode and failure preserving the old destination.
- A 901-second / 86 MB sparse WAV round-trips through `.voice`; an approximately 800 MB, over-one-hour sparse WAV is inspected, sought and read in bounded blocks.
- Decoder tests cover native sample positions, channel preservation, sample-rate conversion and full-stream frame counts. Callback/queue tests cover generations, mixing, silence, loop boundaries and bounded backpressure.
- Scoring tests cover target silence, octave errors, sparse capture, duration bins, seek/resume, excluded capture tail and persisted take summaries.

## Runtime checks

The native GPUI window was launched and inspected at normal and zoomed window sizes. The target piano roll, waveform overview, note labels, transport and secondary Mix/Audio panels rendered in English. Cmd+O opened a self-contained project with two packaged tracks and cached analysis. Enter returned to the beginning, Cmd+1/Cmd+3 opened secondary panels, and Cmd+S saved the modified cursor. The saved archive was independently read: cursor 0, both tracks present, both originals matching SHA-256 against the imported files. The unsaved-changes guard appeared before quitting.

The headless `project-smoke` executable also completed analysis → save → reopen → exact metadata and original-byte comparisons for target-only, target-plus-backing and polyphonic projects. Generated test audio and `.voice` outputs live outside the repository under the task output directory. No demo recording is bundled or offered by the active UI.

Hardware playback could not be verified on this Mac: CPAL output opening stalled for both the default Scarlett and Mac mini Speakers. Independent Apple's `/usr/bin/afplay` on a generated silent WAV also failed to complete in an 8-second bounded probe. CPAL 0.18.2 device property queries succeed; output opening remains below an unresponsive native HAL call, despite a requested 3-second stream build timeout. No OS audio service was reset, and no microphone permission or physical capture was opened during validation. This environment failure is recorded separately from the passing decoder/mixer/actor tests. Do not treat native playback, live singing, monitoring or physical round-trip latency as verified.

A dedicated audio owner thread keeps all hardware calls away from GPUI. Tests prove that a stalled owner does not block construction, polling, pause, shutdown or project work. The UI reports a device that remains unresponsive rather than freezing its window. CUA pointer targeting/occluded-frame capture was unreliable after native dialogs; native picker controls, keyboard actions and window resize provided the observed UI evidence.

## Remaining limits

Polyphonic candidates are not identified choir sections. The reference line requires review. Audio sample-rate conversion uses linear interpolation. Source decoding and PCM buffers are bounded, while analysis/history metadata grows with duration. `.voice` extraction and atomic replacement need local disk space. Packaging is ad hoc signed for local use; Developer ID signing/notarization and Intel Mac validation are outside this local proof of concept.
