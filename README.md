# Voice

A local vocal practice application for macOS, written in Rust with GPUI.

Import a target recording, optionally add a backing track, choose an audio input, and follow the target notes while singing. Projects save as self-contained `.voice` files with original audio, analysis, settings and practice history.

## Run

Requires macOS, Xcode with the Metal toolchain, and Rust 1.100.0. The pinned toolchain is installed automatically by rustup when available.

```sh
# If Xcode reports a missing Metal compiler:
xcodebuild -downloadComponent MetalToolchain

scripts/bundle.sh --debug
open dist/Voice.app
```

For an optimized local build, run `scripts/bundle.sh` without `--debug`. No Node, Python or network connection is needed to run the finished application. Python 3 is used by the packaging script to read Cargo metadata, not by the app. The bundled polyphonic model and font are embedded in the executable. A build downloads dependencies and the native ONNX runtime, which is statically linked on the validated Apple Silicon configuration.

## Use

- Import a target or drop an audio file into the window. Drop two files to use the first as target and the second as backing. Import or remove the optional backing in Mix.
- Select **One voice** for an isolated monophonic recording. Select **Polyphonic** for simultaneous voices/notes. In Target, choose a register and highest/lowest line, listen and confirm the reviewed line before scoring.
- Open Audio, choose devices, an input channel and requested buffer, then apply. Enable the microphone explicitly. Monitoring starts off; use headphones when enabling it.
- Click the pitch view or waveform to seek. Click A and B at the desired cursor positions to set a loop. Adjust the independent gains and mutes in Mix.
- Space plays/pauses; Enter returns to the beginning; arrows seek one second; L toggles looping. Cmd+I imports, Cmd+O opens, Cmd+S saves, Cmd+Shift+S saves as, and Cmd+N starts a new project. Cmd+1–4 opens Mix, Target, Audio and History; Cmd+/ shows shortcuts.
- Save before closing to retain practice history. Reopening a `.voice` file uses its packaged originals and cached analysis, even if the imported source files moved. Monitoring is disabled again on open.

There are no fixed audio-duration or file-size caps. Playback and analysis decode bounded blocks. Metadata grows with duration, and saving/opening needs sufficient disk space. Full albums take longer to analyze. Import analysis can be canceled without replacing the current target.

## Accuracy and limits

The target melody is an estimate. Polyphonic transcription does not identify singers or choir sections and does not separate stems. Selecting a practice line requires human review; note centers are used for polyphonic scoring. The live microphone detector expects one voice. Audio driver requests may fall back; Audio shows observed callback sizes. Timing correction is manual, not a physical round-trip measurement. Playback rate conversion currently uses streaming linear interpolation; matched-rate playback is unchanged.

See [native analysis](docs/NATIVE_ANALYSIS.md), [native benchmark](docs/NATIVE_BENCHMARK.md), [validation](docs/VALIDATION.md), [project format](docs/VOICE_FORMAT.md) and [specification](docs/SPEC.md) for evidence and boundaries.

## Development

```sh
cargo fmt --all --check
cargo test --no-default-features --lib
cargo check --bin voice
cargo clippy --all-targets -- -D warnings
cargo build --bin voice
```

Source responsibilities:

| Module | Responsibility |
| --- | --- |
| `src/desktop/` | GPUI presentation, commands and background jobs |
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
