# Voice macOS v0.4

## Purpose

A local vocal practice application built in Rust with GPUI and macOS platform integrations. The main document window centers on following a vocal part while singing. There is no account, network service, demo song or marketing screen.

| UI term | Meaning |
| --- | --- |
| Vocal reference | A recording of the vocal part to learn |
| Accompaniment | Optional music to sing along with |
| Your voice | Live microphone pitch |

## Workflow

1. Create a project or open a `.voice` file through File → Open, Open Recent, or the macOS project-file association.
2. Import a vocal reference with File → Import Vocal Reference or drag and drop. Explicitly choose One Voice or Several Voices or Notes before analysis. Accompaniment is optional and can be added or removed independently. With two dropped audio files, the first is the reference and the second is accompaniment.
3. For several voices or notes, inspect all gray candidate notes, select higher/lower notes within a register, listen and confirm the blue reference line before scoring. Selection changes invalidate confirmation. Candidates do not identify singers or choir sections.
4. Open the separate Audio Settings window with Voice → Settings or Cmd+,. Choose input/output devices, input channel, gain and requested buffer, then apply. The window reports actual buffers and stream diagnostics.
5. Turn on the microphone explicitly. Check authorization before starting capture; request macOS access only when undetermined. Denied access offers a System Settings link; restricted access explains the restriction. Refresh permission state on activation. Playback and project work remain available without microphone access. Monitoring starts off and is reset on project open or input restart.
6. Play, pause, seek, repeat an A–B loop and adjust independent gains/mutes. Space toggles playback, Enter returns to the beginning, arrows seek one second and L toggles looping. The pitch canvas shows note names, live cents error and the next interval. Scores and take history use the selected reviewed line.
7. Save a self-contained `.voice` document, then reopen it without the original source paths. Before New, Open, Close or Quit discards unsaved changes, present Save / Don't Save / Cancel. Continue the pending action only after a successful save or explicit discard. Cancel and save failure preserve the open document.

## Interface

All active UI, source and documentation are English. Use the macOS system font and light/dark window appearance. The pitch canvas is the primary surface; a compact transport, legend, waveform overview and tuning/status row support it. Blue identifies the vocal reference; orange identifies your voice. Gray polyphonic candidates stay subordinate.

Use the native menu bar for Voice, File, Edit, Playback, View, Window and Help commands. Open/Save panels are AppKit sheets attached to the document's NSWindow, with audio/`.voice` filters and a default `.voice` extension. Standard document chrome reflects the represented file and edited state; opened/saved files populate Open Recent. About uses the standard macOS panel.

Audio Settings is a separate native window. Mix, vocal reference review, history and help are GPUI document overlays. The musical canvas and application controls are rendered by GPUI; file dialogs, menu bar, permission prompt and document alerts use macOS integrations. Follow the patterns in [the interface system](../.interface-design/system.md).

Errors remain visible. Long operations show progress and allow cancellation where safe; failed or canceled analysis preserves the existing reference. Dialogs serialize document actions and restore focus after completion.

## Architecture

- GPUI owns document presentation and input. AppKit provides attached file sheets, document state, recent files and standard panels; AVFoundation provides microphone authorization. File decoding, analysis and project I/O run away from the render loop.
- A retained AppKit delegate proxy intercepts external native termination without borrowing GPUI synchronously; all other GPUI delegate methods are forwarded. A document lifecycle state machine coordinates unsaved-change prompts, asynchronous saves and pending New/Open/Close/Quit actions. Native dialogs do not start duplicate operations.
- CPAL 0.18.2 owns native input/output streams. A separate owner thread contains all hardware initialization and reconfiguration; the UI uses a nonblocking actor. A common output clock mixes two tracks. Audio callbacks use bounded lock-free queues; no file access or decoder work occurs in callbacks.
- Symphonia provides seekable block decoding. There are no product duration or file-size caps. PCM memory is bounded; analysis and history metadata grow with recording length.
- Rust YIN estimates one fundamental frequency. Spotify Basic Pitch ONNX runs locally for polyphonic candidates. No Node, browser or Python runtime is required by the application.
- `.voice` is a versioned ZIP64 archive. Media is streamed and stored without recompression; JSON holds analysis and state. Saves replace the destination atomically after a successful write. Extraction validates paths and required entries.

## Acceptance criteria

Native macOS build and launch; system typography and appearance; menu shortcuts; attached Open/Save sheets; separate Audio Settings; Save/Don't Save/Cancel protection including save failure; permission before capture and denied-access recovery; optional accompaniment; device/channel/buffer selection; independent gains/mutes; seek/play/pause/loop; reference/live plot; reviewed polyphonic score gate; project round-trip with original bytes and cached analysis/history; cancellation/errors preserving prior state; bounded media processing and tests above the former browser limits.

These criteria describe behavior, not an accuracy result. Runtime evidence belongs in [VALIDATION.md](VALIDATION.md); analysis quality and native benchmark conditions are documented separately. Historical web benchmarks remain under `Research/web-v0.2/` and do not establish native accuracy.

## Boundaries

Polyphonic transcription does not isolate stems or identify choir sections. A selected provisional line needs human review, and polyphonic scoring uses estimated note centers. Live pitch expects a single voice. Native driver requests can fall back to a supported configuration; the UI reports observed buffers. Latency compensation is manual rather than a measured round trip. Original recordings and projects stay local. There are no fixed duration/file-size caps, but analysis/history metadata and disk requirements grow with duration. Saving needs temporary space for a second archive; opening extracts media to a private temporary workspace. Local bundles are ad hoc signed, not notarized distribution packages.
