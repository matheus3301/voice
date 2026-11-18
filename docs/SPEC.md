# Voice desktop v0.3

## Purpose

A local, native vocal practice workbench built in Rust with GPUI. Import a target recording, optionally add a backing track, and sing while following the target notes and live microphone pitch. The main window shows one clear musical task. There is no account, network service, demo song, or marketing screen.

## Workflow

1. Create a project or open a `.voice` file.
2. Import a target. Choose monophonic analysis for one isolated voice or polyphonic analysis for simultaneous notes. A backing track is optional.
3. For polyphony, select a register and highest/lowest line, inspect the result, and explicitly confirm it before scoring. Candidate notes are estimates, not identified singers or choir sections.
4. Choose an input/output device, input channel and requested buffer size in Audio. Enable the microphone explicitly; monitoring starts off. Observe the actual callback buffer and stream diagnostics.
5. Play, pause, seek, repeat a loop and adjust the mix. Space toggles playback; Enter returns to the start. The pitch view shows note names, live tuning error and intervals.
6. Save a `.voice` project containing originals, analysis, settings, transport and practice history. Reopen it without depending on the source files.

## Interface

All active UI, source and documentation are English. A restrained graphite workspace, bundled monospace typography, mint target notes and amber live pitch. One primary pitch canvas, a compact waveform overview and transport. Mix, target analysis, audio and history are secondary panels. Errors remain visible and preserve the current project. Long operations show progress and allow cancellation where safe.

## Architecture

- GPUI owns presentation and input. File decoding, analysis and project I/O run away from the render loop.
- CPAL 0.18.2 owns native input/output streams. A separate owner thread contains all hardware initialization and reconfiguration; the UI uses a nonblocking actor. A common output clock mixes two tracks. Audio callbacks use bounded lock-free queues; no file access or decoder work occurs in callbacks.
- Symphonia provides seekable block decoding. There are no product duration or file-size caps. PCM memory is bounded; analysis and history metadata grow with recording length.
- Rust YIN estimates one fundamental frequency. Spotify Basic Pitch ONNX runs locally for polyphonic candidates. No Node, browser or Python runtime is required by the application.
- `.voice` is a versioned ZIP64 archive. Media is streamed and stored without recompression; JSON holds analysis and state. Saves replace the destination atomically after a successful write. Extraction validates paths and required entries.

## Acceptance

Native macOS build and launch; optional backing; hardware selection/channel/buffer; independent gains/mutes; seek/play/pause/loop; clear target/live plot; score gated on reviewed polyphonic targets; project round-trip with original bytes and cached analysis/history; cancellation/errors preserve prior state; streaming tests above the former browser limits; unit tests for pitch, scoring, persistence and transport. Runtime validation must distinguish synthetic evidence from singing accuracy. Historical web benchmarks remain under `Research/web-v0.2/` and do not establish native accuracy.

## Boundaries

Polyphonic transcription does not isolate stems or identify choir sections. A selected provisional line needs human review. Native driver requests can fall back to a supported configuration; the UI reports observed buffers. Latency compensation is manual rather than a measured round trip. Original recordings and project files stay on the local machine. Saving requires temporary disk space for a second archive; opening extracts media to a private temporary workspace.
