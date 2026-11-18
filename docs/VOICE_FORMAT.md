# The `.voice` project format

Voice projects are self-contained ZIP archives. A project stores original audio bytes, reusable analysis, mixer and hardware preferences, transport settings, and practice history. Opening a project does not depend on the original files remaining at their import locations.

The current format version is **1**, represented by `VoiceProject.format_version` in `project.json`. This is independent of the application version and the analysis algorithm version.

JSON parsing enables Serde JSON's `float_roundtrip` feature to preserve the exact `f64` timestamps written by the application, including values separated by a single floating-point unit.

## Archive layout

```text
song.voice
├── project.json
└── media/
    ├── <target-track-id>/
    │   └── <original-target-filename>
    └── <backing-track-id>/
        └── <original-backing-filename>
```

`project.json` contains the serialized `VoiceProject` from `src/model.rs`:

- Project identity, title, format version, and creation/modification timestamps.
- Optional target and backing track records, including identity, role, filename, portable archive path, and audio metadata.
- Track analysis: algorithm version, mono/poly mode, pitch frames, note candidates, waveform peaks, target selection, and review state.
- Mixer settings and audio-device preferences, including input channel, buffer frames, monitor state, gain, and latency compensation.
- Cursor, loop boundaries, loop state, and visible timeline duration.
- Practice takes, including target identity, timing, score, coverage, evaluated duration, and pitch trace.

Backing audio is optional. An empty project can also be saved. Only media declared by the document is included; stale runtime bindings do not create extra archive entries.

Original media entries use ZIP **Stored** compression. They are copied byte for byte rather than decoded, transcoded, or pointlessly recompressed. Entries are written with ZIP64 enabled, so the archive format does not impose the old web prototype's 80 MB or 15-minute limits. File-system capacity, available disk space, and the ZIP implementation's platform capabilities still apply. Metadata uses Deflate compression.

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

An opened workspace extracts declared originals into a private temporary directory. It owns that directory until the workspace is dropped. Keep the workspace alive while playback, analysis, or background saving uses those paths. Imported projects can bind source files directly; creating a new workspace performs no temporary-directory I/O.

`save_project(&document, &media_paths, destination)` writes an immutable background snapshot. The caller must keep the owning workspace and its media alive until saving finishes. The snapshot function does not change timestamps or mutate the caller's document. `ProjectWorkspace::save` supplies an updated modification timestamp and updates its document and `file_path` only after a successful commit.

Original source files should not be modified while a save is in progress. Saving checks the number of copied bytes against the source size and fails if the size changes. This does not detect same-length external edits during copying.

## Atomic saves and bounded audio I/O

Saving validates metadata, creates a temporary sibling of the destination, and streams each original through buffered I/O into the ZIP writer. The implementation does not allocate an audio-sized buffer. Analysis arrays and JSON metadata are already held by the document and are serialized directly into the archive.

After the ZIP central directory is finished, the writer is flushed and the temporary file is synchronized. The temporary file then atomically replaces the destination. A failure before replacement leaves the previous project intact, and the temporary file is cleaned up. Parent-directory synchronization is attempted after replacement on Unix; it is best effort because not every filesystem supports it. This is not a promise of durability against every possible power-loss or storage-device failure.

Opening streams each declared audio entry into the owned workspace. It verifies that the number of extracted bytes matches the ZIP entry size and reads through the ZIP reader so decompression and CRC errors are surfaced. Neither opening nor saving decodes the complete audio into RAM.

## Validation

Opening rejects unsupported `format_version` values before attempting to deserialize the current document schema. Future versions require an explicit migration path. Unknown fields within the current version are ignored by Serde and are not preserved on a subsequent save; callers must not use them for essential data without defining a format-version policy.

Both saving and opening validate:

- Media roles, nonempty identities and filenames, distinct media IDs, and distinct archive paths.
- Positive audio duration, sample rate, channel count, and frame count, without fixed duration or file-size caps.
- Finite numeric values. Unvoiced pitch frames must use finite sentinels such as zero frequency and zero MIDI, rather than NaN or infinity.
- Frame, note, and waveform timestamps within the associated audio duration, allowing a 25 ms boundary tolerance for sampled analysis.
- Positive note lengths, MIDI values in `0..=127`, confidence in `0..=1`, and valid target-selection bounds.
- Nonnegative gains and RMS, positive pitch tolerance and visible timeline duration, and a transport cursor within the project duration.
- Enabled loop boundaries within the project. Disabled loops may retain their settings before audio is loaded.
- Practice percentages in `0..=100`, finite take bounds, evaluated duration within the take, and trace timestamps within that take. A history entry can refer to a former target, so its bounds are validated against its own take rather than a replacement track's duration.

All archive entry names are checked before extraction. Absolute paths, drive-prefixed paths, traversal components, backslashes, empty components, NULs, symbolic links, and unsupported special-file types are rejected. Media references must point inside `media/`. Extraction uses newly created regular files inside a fresh private directory and never follows project-supplied filesystem references outside that directory. Missing or empty required assets cause opening to fail; incomplete extraction directories are removed automatically. Safe, unreferenced extra entries are not extracted.

Metadata validation establishes structural consistency; it does not prove that an audio decoder accepts an embedded file or that stored analysis is musically correct. Decoding and analysis have their own validation and versioning responsibilities.

## Tests

Focused persistence tests cover metadata/analysis/preferences/history round trips, byte-for-byte original preservation, target-only and empty projects, atomic overwrite and failed-save preservation, extraction ownership, snapshot immutability, unsupported versions, truncated ZIPs, unsafe entry paths, missing assets, Unicode, invalid numeric/time values, and forward-compatible unknown fields.

The large-file test creates a valid sparse PCM WAV with **901 seconds** of audio and more than **80 MiB** of original bytes. It streams that original through save and open, checking its length and content markers without reading it into one buffer. This verifies removal of the former web caps and exercises the streaming path; it does not replace a separate multi-gigabyte performance or crash-recovery benchmark.

The ZIP API was checked against [zip-rs documentation](https://github.com/zip-rs/zip2/tree/v2.4.2), including the library's writing and extraction examples. The dependency is constrained to the compatible `2.x` API by the project's Cargo manifest.
