# Native validation

Validated on Apple Silicon macOS with Rust 1.100.0, GPUI 0.2.2, CPAL 0.18.2 and Symphonia 0.5.5. The release binary builds successfully and the local application bundle is ad hoc signed.

## Automated evidence

- `cargo fmt --all --check`, `cargo check --all-targets`, warning-free Clippy and optimized native build pass.
- Core unit/integration suite: 60 passed; 4 opt-in corpus/hardware tests ignored by the default suite. The corpus benchmark is run separately against the local authorized Vocadito cache.
- Ten document-lifecycle tests cover New/Open/Close/Quit, save gating, cancellation, disk failure, changes during save and stale completions. Two desktop permission-decision tests cover cancellation and authorization changes before completion. Live diagnostics are tested against the actual callback renderer with the hardware owner parked.
- Real bundled ONNX model inference runs natively, including simultaneous-note synthetic input and multi-chunk real singing.
- Project tests cover original-byte round trips, cached analysis, settings/history, future versions, unsafe paths, symlinks, corrupt/truncated files, CRC, ZIP64 headers, Unicode and failure preserving the old destination.
- A 901-second / 86 MB sparse WAV round-trips through `.voice`; an approximately 800 MB, over-one-hour sparse WAV is inspected, sought and read in bounded blocks.
- Decoder tests cover native sample positions, channel preservation, sample-rate conversion and full-stream frame counts. Callback/queue tests cover generations, mixing, silence, loop boundaries and bounded backpressure.
- Scoring tests cover target silence, octave errors, sparse capture, duration bins, seek/resume, excluded capture tail and persisted take summaries.

## Runtime checks

Version 0.4 was tested in an isolated, ad hoc signed `Voice Preview.app` so the user's running Voice session and real project remained untouched. Native menus expose File/Open/Save/Save As/import commands and current terminology. The document uses the system font, neutral dark system appearance, a blue vocal reference, orange live pitch and a separate Audio Settings window. Light appearance has a dedicated palette but was not toggled during this runtime pass.

Attached AppKit Open and Save sheets were observed in the accessibility tree. Opening a two-track fixture restored cached analysis and the pitch/waveform canvas. A fresh vocal-reference import presented the native One Voice / Several Voices or Notes question, then completed analysis. Arrow seeking and Save As produced `Native Practice.voice`; an independent ZIP/JSON read confirmed cursor 1.0, the saved document title and SHA-256-identical original bytes for both tracks. Native title/represented-file state updated after saving. A native Save / Don't Save / Cancel alert appeared before New. Cancel preserved the document; choosing Save and then canceling its Save sheet also preserved the imported reference. CUA must select the native picker's containing file row: clicking only its filename text can show AX selection without enabling Open. Pointer targeting remains unreliable in this environment.

Audio Settings opening/activation is deferred until the document borrow is released, avoiding synchronous native-window reentry. Permission completion is scoped to the document entity, so closing Settings cannot strand a pending request. New Project reapplies its audio defaults. External native quit requests are intercepted by a retained delegate proxy, forwarded into the same save lifecycle, and only permitted after that lifecycle accepts quitting; GPUI's other delegate methods remain forwarded. The final preview completed Save-before-Quit to an existing project and exited normally with the guard installed. The Dock/OS entry point was reviewed and compiled, but was not exercised directly because the CUA Dock surface was unavailable.

The headless `project-smoke` executable also completed analysis → save → reopen → exact metadata and original-byte comparisons for target-only, target-plus-backing and polyphonic projects. Generated test audio and `.voice` outputs live outside the repository under the task output directory. No demo recording is bundled or offered by the active UI.

## Hardware recheck

The first desktop pass encountered a stalled host audio path: both CPAL output creation and an independent eight-second `afplay` probe failed. After the user reported resolving a permission issue, System Settings showed Voice microphone access enabled. A fresh generated-silence `afplay` check completed successfully, and an output-only CPAL controller probe reported a live 256-frame Scarlett callback for ten seconds without a stream error. This also exposed and led to a fix for stale UI diagnostics: callback sizes/errors/drops now refresh from live atomics without hardware queries.

The later preview run still reported an unresponsive audio device while the user's original Voice session was open. No system audio service was reset or active user session interrupted. Host output initialization therefore has successful evidence but is not consistently available in this environment. Audible musical playback, sung-pitch feedback, monitoring and physical round-trip latency remain unverified. The audio owner thread contains stalled native calls and keeps project work and UI shutdown responsive.

## Remaining limits

Polyphonic candidates are not identified choir sections. The reference line requires review. Audio sample-rate conversion uses linear interpolation. Source decoding and PCM buffers are bounded, while analysis/history metadata grows with duration. `.voice` extraction and atomic replacement need local disk space. Packaging is ad hoc signed for local use; Developer ID signing/notarization and Intel Mac validation are outside this local proof of concept.
