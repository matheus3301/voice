# The `.voice` project format

Voice projects are self-contained ZIP archives. A project stores its current reference/accompaniment originals, recorded take audio, reusable analysis, mixer settings, transport and practice history. Opening does not depend on imported files remaining at their original locations.

The current format version is **2**, represented by `VoiceProject.format_version` in `project.json`. Voice v0.5 reads v1 through migration and writes v2. The format version is independent of the application and analysis versions. Earlier v1-only applications reject v2 instead of silently dropping recorded takes.

JSON parsing enables Serde JSON's `float_roundtrip` feature to preserve the exact `f64` timestamps written by the application, including values separated by a single floating-point unit.

## Archive layout

```text
song.voice
├── project.json
├── media/
│   ├── <target-track-id>/
│   │   └── <original-target-filename>
│   └── <backing-track-id>/
│       └── <original-backing-filename>
└── recordings/
    └── <practice-take-id>.wav
```

`project.json` contains the serialized `VoiceProject` from `src/model.rs`:

- Project identity, title, format version, and creation/modification timestamps.
- Optional target and backing track records, including identity, role, filename, portable archive path, and audio metadata.
- Track analysis: algorithm version, mono/poly mode, pitch frames, note candidates, waveform peaks, target selection, and review state.
- Mixer settings: reference/accompaniment gains and mutes, a separate `monitor_gain` for Your voice, pitch tolerance, and an audio-preferences compatibility snapshot.
- Cursor, loop boundaries, loop state, and visible timeline duration.
- Practice takes: reference identity, timing, stored score/coverage/evaluated duration, pitch trace, optional reference-line selection snapshot, and optional recording metadata.

`PracticeTake.recording` is optional and defaults to absent. When present, `TakeRecording` contains `archive_path`, `sample_rate` and `frames`; duration is `frames / sample_rate`. Its path must be exactly `recordings/<take.id>.wav`. Recordings use mono signed PCM32 WAV, or RF64 for sizes beyond RIFF's range. The file contains the selected microphone channel after capture gain, before the Your voice monitoring/replay fader. Imported originals are never transcoded for persistence.

The optional `reference_selection` snapshot identifies the line used by an evaluated polyphonic take. Replay comparison requires the same current reference identity and that line selection. Stored results are not recomputed using a new tolerance. A trace-only take has no recoverable audio; an audio take may have no evaluated score. Replacing a reference does not archive the removed reference as a new media slot, so keep the original project when retaining a comparison against it matters.

Backing audio is optional. An empty project can also be saved. Only media declared by the document is included; stale runtime bindings do not create extra archive entries.

Original media and take entries use ZIP **Stored** compression and preserve their bytes. Entries are written with ZIP64 enabled, so the archive format does not impose the old web prototype's 80 MB or 15-minute limits. RF64 is the large recording's audio container; ZIP64 is the surrounding project archive format. Filesystem capacity, disk space and platform capabilities still apply. Metadata uses Deflate compression.

Unicode filenames and titles are supported. The display filename is metadata; `archive_path` is a separate portable, relative path under `media/`. The application must construct archive paths with forward slashes, for example `media/<id>/<filename>`.

## Workspace ownership

`ProjectWorkspace` separates serializable document data from runtime filesystem paths:

```rust
let mut workspace = ProjectWorkspace::new();
workspace.document = project;
workspace.bind_media(TrackRole::Target, source_path);
workspace.save(destination)?;

let reopened = ProjectWorkspace::open(destination)?;
let target_path = &reopened.media_paths[&TrackRole::Target];
```

An opened workspace extracts declared originals and recordings into a private temporary directory. It owns that directory until the workspace is dropped. `media_paths` maps track roles to runtime paths; `recording_paths` maps portable recording paths to runtime paths. Keep the workspace alive during playback, analysis and background saves. Creating a new workspace performs no temporary-directory I/O.

`recording_destination(take_id)` lazily creates a separate owned temporary directory for new captures and returns a guarded destination. After successful recording finalization, attach `TakeRecording` to the history entry and bind its archive path in `recording_paths`. Failed/pending captures are not silently added as successful takes. These temporary recordings remain workspace-owned until packaged and/or the workspace is dropped.

`save_project_with_recordings(&document, &media_paths, &recording_paths, destination)` writes an immutable background snapshot. Keep the owning workspace and its files alive until completion. `save_project` remains a wrapper for documents without recordings; it fails if required recording bindings are absent. Snapshot functions do not mutate the document or timestamps. `ProjectWorkspace::save` updates the modification timestamp and commits document/`file_path` only after success.

Original source files should not be modified while a save is in progress. Saving checks the number of copied bytes against the source size and fails if the size changes. This does not detect same-length external edits during copying.

## Atomic saves and bounded audio I/O

Saving validates metadata, creates a temporary sibling of the destination, and streams originals and completed recordings through buffered I/O into the ZIP writer. It does not allocate an audio-sized buffer. Analysis arrays and JSON metadata are already held by the document and are serialized directly. The desktop waits for pending take finalization before saving or replacing/closing its document.

After the ZIP central directory is finished, the writer is flushed and the temporary file is synchronized. The temporary file then atomically replaces the destination. A failure before replacement leaves the previous project intact, and the temporary file is cleaned up. Parent-directory synchronization is attempted after replacement on Unix; it is best effort because not every filesystem supports it. This is not a promise of durability against every possible power-loss or storage-device failure.

Opening streams each declared audio entry into the owned workspace. It verifies that the number of extracted bytes matches the ZIP entry size and reads through the ZIP reader so decompression and CRC errors are surfaced. Neither opening nor saving decodes the complete audio into RAM.

## Validation

Opening checks `format_version` before interpreting the full document. A v1 project gains default `monitor_gain = 1`, absent recording/selection metadata and in-memory version 2; its next save writes v2. A v1 manifest containing recording metadata is rejected. Unsupported versions are rejected. Unknown fields in supported schemas are ignored and not preserved on save, so essential new data requires an explicit version policy.

Both saving and opening validate:

- Media roles, nonempty identities and filenames, distinct media IDs, and distinct archive paths.
- Positive audio duration, sample rate, channel count, and frame count, without fixed duration or file-size caps.
- Finite numeric values. Unvoiced pitch frames must use finite sentinels such as zero frequency and zero MIDI, rather than NaN or infinity.
- Frame, note, and waveform timestamps within the associated audio duration, allowing a 25 ms boundary tolerance for sampled analysis.
- Positive note lengths, MIDI values in `0..=127`, confidence in `0..=1`, and valid target-selection bounds.
- Nonnegative gains and RMS, positive pitch tolerance and visible timeline duration, and a transport cursor within the project duration.
- Enabled loop boundaries within the project. Disabled loops may retain their settings before audio is loaded.
- Practice percentages in `0..=100`, finite take bounds, evaluated duration within the take, and trace timestamps within that take. A history entry can refer to a former target, so its bounds are validated against its own take rather than a replacement track's duration.
- Recording paths/IDs and unique asset references; positive sample rate/frame count; exact agreement with the canonical mono PCM32 WAV/RF64 reader, including container length, block alignment, byte rate, RF64 data/sample counts and complete PCM extent. Header inspection seeks/reads bounded data without loading PCM.

All archive entry names are checked before extraction. Absolute/drive-prefixed paths, traversal, backslashes, empty components, NULs, symbolic links and unsupported special files are rejected. Original media stays under `media/`; takes stay directly under `recordings/`. Extraction creates regular files inside a fresh private directory and follows no external project paths. Missing/empty required assets fail opening; failed extraction is cleaned up. Safe unreferenced entries are not extracted.

Imported-media metadata validation does not prove decoder compatibility or musical correctness. Take PCM has the stricter application-owned format check described above. Analysis accuracy has separate evidence and versioning.

## Machine-local audio preferences

`~/Library/Application Support/Voice/preferences.json` stores versioned `AudioPreferences` independently of `.voice` files: input/output names, mono channel, requested buffer, capture gain and manual latency correction. The desktop applies these local choices on launch and when opening/creating projects; a song's historical hardware snapshot does not replace the current Mac's routing.

Preferences use atomic sibling-file replacement. A missing file loads safe defaults; malformed/unsupported data returns an error for the caller to surface. Monitoring is stored/loaded disabled, never automatically restored as audible. The Your voice mix fader is project `monitor_gain`, not global capture `input_gain`. Tests supply explicit disposable paths; `VOICE_PREFERENCES_PATH` allows an isolated desktop validation path.

## Tests

Focused tests cover original/recorded byte preservation, PCM32/RF64 validation, v1 migration, recording-path guards, missing/mismatched assets, ZIP64/Stored entries, metadata/history round trips, atomic failure preservation, extraction lifetime, Unicode, invalid numeric/time values and unknown fields. Separate preferences tests cover defaults, remembered hardware, monitoring reset and invalid-write preservation.

The large-file test creates a valid sparse PCM WAV with **901 seconds** of audio and more than **80 MiB** of original bytes. It streams that original through save and open, checking its length and content markers without reading it into one buffer. This verifies removal of the former web caps and exercises the streaming path; it does not replace a separate multi-gigabyte performance or crash-recovery benchmark.

The ZIP API was checked against [zip-rs documentation](https://github.com/zip-rs/zip2/tree/v2.4.2), including the library's writing and extraction examples. The dependency is constrained to the compatible `2.x` API by the project's Cargo manifest.
