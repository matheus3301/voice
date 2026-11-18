# Voice v0.4

A local vocal practice application for macOS, written in Rust with GPUI.

Import a **Vocal reference**, optionally add **Accompaniment**, and follow the reference notes alongside **Your voice** from the microphone. The document window uses system typography and follows macOS light/dark appearance. File commands live in the native menu bar; Open and Save use sheets attached to the document window.

Projects save as self-contained `.voice` files with original audio, analysis, settings, transport and practice history.

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

## Use

1. Choose **File → Import Vocal Reference…** or drop a recording into the window. The vocal reference is the recording of the part you want to learn. Choose **One Voice** for an isolated voice or **Several Voices or Notes** for layered harmonies or simultaneous notes.
2. Optionally choose **File → Import Accompaniment…** for music to sing along with. Dropping two files uses the first as vocal reference and the second as accompaniment. Mix provides independent gains/mutes and accompaniment removal.
3. For a reference containing several voices or notes, open **Reference…**, choose higher/lower notes and a note range, listen, then select **Use this reference line** before scoring. Changing the selection requires review again.
4. Choose **Voice → Settings…** (**Cmd+,**) to open the separate **Audio Settings** window. Choose input/output devices, an input channel and requested buffer, then apply. Turn on the microphone explicitly: Voice requests macOS permission before capture. If access was denied, use **Open System Settings…**, enable Voice under **Privacy & Security → Microphone**, return and turn it on. Recordings can still be opened and played without microphone access. Monitoring starts off; use headphones when enabling it.
5. Click the pitch view or waveform to seek. Click **A** and **B** at the desired cursor positions to set a loop. The canvas shows reference notes in blue and your live pitch in orange, with note names, cents and the next interval.
6. Choose **File → Save** to retain the project and practice history. New, Open, Close and Quit offer **Save / Don't Save / Cancel** when the document has unsaved changes. A canceled or failed save leaves the document open. Reopening a `.voice` file uses its packaged originals and cached analysis, even if the source files moved. Monitoring is disabled again on open.

| Shortcut | Action |
| --- | --- |
| Space / Enter | Play or pause / return to beginning |
| Left / Right / L | Seek one second / toggle loop |
| M | Turn microphone on or off |
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

See [native analysis](docs/NATIVE_ANALYSIS.md), [native benchmark](docs/NATIVE_BENCHMARK.md), [validation](docs/VALIDATION.md), [project format](docs/VOICE_FORMAT.md) and [specification](docs/SPEC.md) for evidence and boundaries.

## Development

```sh
cargo fmt --all --check
cargo test --no-default-features --lib
cargo test --bin voice permission_tests
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
| `src/analysis.rs` | YIN and local Basic Pitch inference |
| `src/scoring.rs` | Duration-based scoring and capture tail |
| `src/project.rs` | Atomic ZIP64 persistence and safe extraction |
| `src/model.rs` | Versioned portable document |

All active code, UI text, comments and documentation are English. Commits use Conventional Commits in English.

## Research archive

The previous web application, market research, papers, benchmark reports and scripts are preserved unchanged under [`Research/web-v0.2/`](Research/web-v0.2/). This is a historical snapshot, not a runtime dependency of the desktop application. Historical benchmark results are not automatically evidence for a native implementation.

Third-party licenses and model provenance are included under `assets/`.
