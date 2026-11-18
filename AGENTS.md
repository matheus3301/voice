# Voice development

- All active code, comments, UI strings and documentation are written in English.
- Every commit uses Conventional Commits in English: `type(scope): description`. This includes research, documentation and Knowledge Base commits.
- Preserve `research/web-v0.2/` as a historical snapshot. Do not develop the active application there.
- This is a native Rust + GPUI application. Keep decoding and analysis off the UI thread and allocation/file I/O out of audio callbacks.
- Do not add fixed audio-duration or file-size limits. Use bounded streaming, seekable media and ZIP64 project storage.
- A `.voice` project must be self-contained: original audio, versioned metadata, reusable analysis, settings and practice history. Save atomically.
- Run formatting, core tests, native build and focused runtime checks for changes. Keep research benchmarks distinct from unit-test results.
