# Voice macOS v0.5

## Purpose

A local vocal practice application built in Rust with GPUI and macOS platform integrations. The main document window centers on following a vocal part while singing. There is no account, network service, demo song or marketing screen.

| UI term | Meaning |
| --- | --- |
| Vocal reference | A recording of the vocal part to learn |
| Accompaniment | Optional music to sing along with |
| Your voice | Live microphone pitch |

## Workflow

1. Create a project or open a `.voice` file through File → Open, Open Recent, or the macOS project-file association.
2. Stage a required vocal reference and optional accompaniment in separate file/drop fields. Choose One voice or Several voices or notes, then Continue to start analysis. Replacing/removing either staged file does not start analysis. File imports remain available for later changes.
3. For several voices or notes, inspect all gray candidate notes, select higher/lower notes within a register, listen and confirm the blue reference line before scoring. Selection changes invalidate confirmation. Candidates do not identify singers or choir sections.
4. Choose input/output devices and an available mono input channel in the persistent Mixer sidebar. Remember these choices, requested buffer, capture gain and manual timing correction in machine-local preferences, independently of a project. Use the separate Audio Settings window (Advanced… / Cmd+,) for detailed configuration and observed buffer/stream diagnostics. Project open/new keeps the current Mac's routing.
5. Turn on the microphone explicitly. Check authorization before starting capture; request macOS access only when undetermined. Denied access offers a System Settings link; restricted access explains the restriction. Refresh permission state on activation. Playback and project work remain available without microphone access. Monitoring starts off and is reset on project open or input restart.
6. Play, pause, seek, repeat an A–B loop and balance Your voice, Vocal reference and Accompaniment with independent faders/meters. Your voice is a post-capture monitoring/replay gain; input gain affects capture and analysis. Mic, Hear myself and Record take are distinct actions. Space toggles playback, Enter returns to the beginning, arrows seek one second and L toggles looping.
7. Record a take from the selected mono input while playback runs. Suspend looping for a single pass and stop/finalize on transport interruption. A bounded writer stores PCM32 WAV, switching to RF64 for large recordings. Capture overflow/file failure produces an error rather than a successful incomplete take. Saving or document transitions wait for finalization.
8. Listen to a stored take, display its recorded pitch trace and stored result, and restore the previous cursor afterward. Do not recompute recorded results with current tolerance. Comparison requires the same reference identity and, for evaluated polyphonic takes, the saved line selection. A take can contain audio without an evaluated score; older pitch-only history cannot be replayed as audio.
9. Save a self-contained `.voice` document containing original audio and take recordings. Read/migrate format v1; write format v2 so old applications reject recording-bearing files. Before New/Open/Close/Quit discards changes, present Save / Don't Save / Cancel. Continue only after successful save or explicit discard; cancel/failure preserves the document.

## Interface

All active UI, source and documentation are English. Use the macOS system font and light/dark window appearance. The pitch canvas is the primary surface; a compact transport, legend, waveform overview and tuning/status row support it. Blue identifies the vocal reference; orange identifies your voice. Gray polyphonic candidates stay subordinate.

Use the native menu bar for Voice, File, Edit, Playback, View, Window and Help commands. Open/Save panels are AppKit sheets attached to the document's NSWindow, with audio/`.voice` filters and a default `.voice` extension. Standard document chrome reflects the represented file and edited state; opened/saved files populate Open Recent. About uses the standard macOS panel.

The persistent Mixer sidebar combines device/channel selectors, Mic, Hear myself, Record take, latest-take playback and three faders/meters. It can be hidden to expand the musical canvas. Audio Settings is a separate native window; reference review, history and help remain GPUI overlays. The musical canvas and application controls are rendered by GPUI; file dialogs, menu bar, permission prompt and document alerts use macOS integrations. Follow [the interface system](../.interface-design/system.md) and [documented DAW setup precedents](DAW_SETUP.md).

Errors remain visible. Long operations show progress and allow cancellation where safe; failed or canceled analysis preserves the existing reference. Dialogs serialize document actions and restore focus after completion.

## Architecture

- GPUI owns document presentation and input. AppKit provides attached file sheets, document state, recent files and standard panels; AVFoundation provides microphone authorization. File decoding, analysis and project I/O run away from the render loop.
- A retained AppKit delegate proxy intercepts external native termination without borrowing GPUI synchronously; all other GPUI delegate methods are forwarded. A document lifecycle state machine coordinates unsaved-change prompts, asynchronous saves and pending New/Open/Close/Quit actions. Native dialogs do not start duplicate operations.
- CPAL 0.18.2 owns native input/output streams. A separate owner thread contains hardware initialization and reconfiguration; the UI uses a nonblocking actor. A common output clock mixes reference/accompaniment playback and the monitoring/replay bus. Audio callbacks use bounded lock-free queues; no file access or decoding occurs there.
- A bounded recording queue transfers selected, capture-gained mono samples to a WAV/RF64 writer thread. A separate reader streams take playback. Live monitoring and replay share the Your voice fader without changing stored PCM. Meters read cached callback values rather than querying hardware during rendering.
- Machine-local JSON audio preferences are written atomically by a background writer. Monitoring is never restored as enabled. Serialized project audio settings remain a compatibility snapshot; current local preferences determine active routing.
- Symphonia provides seekable block decoding. There are no product duration or file-size caps. PCM memory is bounded; analysis and history metadata grow with recording length.
- Rust YIN estimates one fundamental frequency. Spotify Basic Pitch ONNX runs locally for polyphonic candidates. No Node, browser or Python runtime is required by the application.
- `.voice` is a versioned ZIP64 archive. Media is streamed and stored without recompression; JSON holds analysis and state. Saves replace the destination atomically after a successful write. Extraction validates paths and required entries.

## Acceptance criteria

Native macOS build and launch; deferred two-file setup; native menus/sheets; separate Audio Settings; unsaved-change protection; permission before capture; machine-local routing; valid mono channel selection; persistent faders/meters; independent capture and monitoring; record/finalize/listen with stored trace/result; reference identity/selection replay guards; v1 migration and v2 archive round-trip preserving original/take bytes; cancellation/errors preserving prior state; bounded media processing without fixed audio caps.

These criteria describe behavior, not an accuracy result. Runtime evidence belongs in [VALIDATION.md](VALIDATION.md); analysis quality and native benchmark conditions are documented separately. Historical web benchmarks remain under `Research/web-v0.2/` and do not establish native accuracy.

## Boundaries

Polyphonic transcription does not isolate stems or identify choir sections. A selected line needs human review; scoring uses estimated note centers. Live pitch expects one voice. Native driver requests may fall back; timing correction is manual rather than a measured round trip. Voice does not provide full DAW routing, plug-ins, punch editing or a latency guarantee. Audio/projects remain local. Metadata and disk needs grow with duration despite bounded PCM memory and no fixed audio caps. Saving needs temporary space for another archive; opening extracts owned temporary media. Local bundles are ad hoc signed, not notarized distribution packages.
