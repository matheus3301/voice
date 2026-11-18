# Voice v0.5

A local vocal practice application for macOS, written in Rust with GPUI.

Import a **Vocal reference**, optionally add **Accompaniment**, and follow the reference notes alongside **Your voice** from the microphone. The document window uses system typography and follows macOS light/dark appearance. File commands live in the native menu bar; Open and Save use sheets attached to the document window.

Projects save as self-contained `.voice` files with original audio, recorded takes, analysis, settings, transport and practice results. Device preferences are remembered separately on this Mac.

## Run

Requires macOS, Xcode with the Metal toolchain, and Rust 1.100.0. The pinned toolchain is installed automatically by rustup when available.

```sh
# If Xcode reports a missing Metal compiler:
xcodebuild -downloadComponent MetalToolchain

scripts/bundle.sh --debug
open dist/Voice.app
```

For an optimized local build, run `scripts/bundle.sh` without `--debug`. Launch the `.app` bundle so macOS can identify Voice for microphone permission and project files. The local bundle is signed ad hoc.

No Node, Python or network connection is needed to run the finished application. Python 3 is used by the packaging script to read Cargo metadata. The polyphonic model is embedded in the executable. Building downloads dependencies and the native ONNX runtime, which is statically linked on the validated Apple Silicon configuration.

For an isolated review build, use `scripts/bundle.sh --debug --preview`. It produces `build/Voice Preview.app` with a separate app identity and leaves the normal bundle and its project association alone.
Set `VOICE_PREFERENCES_PATH` to a disposable JSON path when testing routing without changing the normal machine preferences.

## Use

1. Stage a **Vocal reference** and optional **Accompaniment** in the two audio fields. Choose files or drop them into their fields, select **One voice** or **Several voices or notes**, then **Continue** to analyze. The reference is the recorded vocal part you want to learn; accompaniment is music to sing along with. File-menu imports remain available when replacing or adding audio later.
2. For a reference containing several voices or notes, open **Reference…**, choose higher/lower notes and a note range, listen, then select **Use this reference line** before scoring. Changing the selection requires review again.
3. Use the persistent **Mixer** sidebar to choose input/output devices and the actual mono microphone channel. Three faders balance **Your voice**, **Vocal reference** and **Accompaniment**, with level meters. **Your voice** changes monitoring and take playback, independently of capture/input gain. Use **Advanced…** or **Voice → Settings…** (**Cmd+,**) for buffer, input gain and timing correction. Routing preferences survive app launches and project changes.
4. Turn on **Mic** for live pitch. Voice requests macOS permission before capture; denied access offers **Open System Settings…**. Enable Voice under **Privacy & Security → Microphone**, return and turn it on. **Hear myself** separately enables monitoring, which starts off. Use headphones; leave it off when monitoring directly through the interface.
5. Click the pitch view or waveform to seek. Click **A** and **B** at the desired cursor positions to set a loop. The canvas shows reference notes in blue and your voice in orange, with note names, cents and the next interval. Choose **Record take** to capture your microphone while the song plays, then stop and wait for finalization. Looping is suspended during a recording.
6. Choose **Listen** in Takes or **Listen to take** in the sidebar to hear recorded PCM and inspect its saved trace/result. Results are stored, not recomputed using a later tolerance. Comparison requires the original vocal reference and, for a scored polyphonic take, its original line selection. Older pitch-only history has no audio to play.
7. Choose **File → Save** to package originals, recordings and history. New, Open, Close and Quit offer **Save / Don't Save / Cancel** for unsaved changes. Cancel or save failure leaves the document open. Format v1 projects migrate on open; new saves use format v2. Monitoring is disabled on launch/open/input restart, even when devices are remembered.

| Shortcut | Action |
| --- | --- |
| Space / Enter | Play or pause / return to beginning |
| Left / Right / L | Seek one second / toggle loop |
| M / Option+M | Turn microphone / monitoring on or off |
| R | Start or stop a recorded take |
| Cmd+I / Cmd+Shift+I | Import vocal reference / accompaniment |
| Cmd+O / Cmd+N | Open / new project |
| Cmd+S / Cmd+Shift+S | Save / Save As |
| Cmd+W / Cmd+Q | Close document / quit |
| Cmd+, | Audio Settings |
| Cmd+1 / Cmd+2 / Cmd+4 | Mix / Vocal Reference / Practice History |
| Cmd+/ | Keyboard shortcuts |

There are no fixed audio-duration or file-size caps. Playback and analysis decode bounded blocks. Metadata grows with duration, and saving/opening needs sufficient disk space. Full albums take longer to analyze. Import analysis can be canceled without replacing the current vocal reference.

## Accuracy and limits

Reference note detection is an estimate. Polyphonic transcription does not identify singers or choir sections and does not separate stems. Selecting a practice line requires human review; note centers are used for polyphonic scoring. The live microphone detector expects one voice. Audio driver requests may fall back; Audio Settings shows observed callback sizes. Timing correction is manual, not a physical round-trip measurement. Playback rate conversion currently uses streaming linear interpolation; matched-rate playback is unchanged.

See [DAW setup research](docs/DAW_SETUP.md), [native analysis](docs/NATIVE_ANALYSIS.md), [native benchmark](docs/NATIVE_BENCHMARK.md), [validation](docs/VALIDATION.md), [project format](docs/VOICE_FORMAT.md) and [specification](docs/SPEC.md) for evidence and boundaries.

## Development

```sh
cargo fmt --all --check
cargo test --no-default-features --lib
cargo test --bin voice
cargo check --bin voice
cargo clippy --all-targets -- -D warnings
cargo build --bin voice
```

Source responsibilities:

| Module | Responsibility |
| --- | --- |
| `src/desktop/` | Document workspace, system appearance, menu bar, native sheets, settings and background jobs |
| `src/document_lifecycle.rs` | Save/discard/cancel document transitions |
| `src/audio.rs` | Native devices, transport, mixing, capture and monitoring |
| `src/audio_controller.rs` | Hardware owner thread and nonblocking UI control |
| `src/media.rs` | Seekable bounded decoding |
| `src/recording.rs` | Bounded mono PCM32 WAV/RF64 recording, inspection and replay reading |
| `src/preferences.rs` | Atomic machine-local audio preferences |
| `src/analysis.rs` | YIN and local Basic Pitch inference |
| `src/scoring.rs` | Duration-based scoring and capture tail |
| `src/project.rs` | Atomic ZIP64 persistence and safe extraction |
| `src/model.rs` | Versioned portable document |

All active code, UI text, comments and documentation are English. Commits use Conventional Commits in English.

## Research archive

The previous web application, market research, papers, benchmark reports and scripts are preserved unchanged under [`Research/web-v0.2/`](Research/web-v0.2/). This is a historical snapshot, not a runtime dependency of the desktop application. Historical benchmark results are not automatically evidence for a native implementation.

Third-party licenses and model provenance are included under `assets/`.
