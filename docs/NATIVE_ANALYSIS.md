# Native analysis and scoring

The active implementation is Rust. Decoding uses Symphonia, inference uses the statically linked ONNX Runtime through `ort`, and the bundled Spotify Basic Pitch ONNX graph is embedded in the application. No Node, browser, Python process, model server, or runtime model download is involved.

## Interfaces

`analysis::analyze_file(path, mode, cancel, progress)` synchronously performs analysis on the calling thread and returns a versioned `TrackAnalysis`. The application must call it from a background worker. Cancellation is checked during decoding and before each neural inference window. Progress is a fraction from 0 to 1. Cancellation is an error and does not yield a partially accepted target.

`analysis::detect_pitch(samples, sample_rate, time)` is the live-input YIN detector. It allocates temporary difference-function storage and must run on the capture-analysis worker, never on the hardware callback. The input engine provides the already compensated centre timestamp. The scorer must not subtract compensation from that timestamp again.

`analysis::selected_notes` returns a highest/lowest provisional line within the selected register. Its sweep-line implementation is O(n log n) in the number of candidates. `analysis::practice_target` preserves continuous F0 for monophonic analysis; for polyphonic analysis it creates a 25 ms reference grid from the selected note centres. It preserves the review flag and never identifies a singer or voice part.

`scoring::PracticeScorer` exposes `new`, `reset`, `begin_segment`, `advance`, `sample`, `snapshot`, and `to_take`. `ScoreSnapshot` contains score, coverage, cents, target MIDI, matched seconds and evaluated target seconds. `pitch_at` performs bounded nearest-frame lookup. `to_take` copies score and trace into a persistent `PracticeTake`; project storage owns history retention.

## Monophonic detector

The native YIN algorithm retains the historical baseline's difference function, cumulative normalization, 0.15 trough threshold, 0.82 confidence threshold, 0.006 RMS gate, parabolic lag interpolation and three-frame median smoothing. Offline input is box-resampled to 16 kHz. Windows contain 1,024 samples, centred at 25 ms intervals, with zero padding at file boundaries. The accepted range is 65–1,200 Hz. Note segmentation uses rounded MIDI centres and a 70 ms minimum.

The resampler integrates source-sample intervals and retains fractional state across decoder block boundaries. It does not reset its phase every packet. Non-finite source samples are treated as silence. A resampler test compares arbitrary 997-sample partitions against a single input block. Native f32 arithmetic and the native decoder are not asserted to produce bit-identical frame frequencies to the archived JavaScript implementation.

## Bounded decoding and waveform cache

The mono path reads bounded decoder blocks, retains a short resampling buffer and a rolling centred analysis window, and discards raw PCM after processing. Pitch frames and note events are retained as reusable analysis, not full-resolution PCM.

The waveform cache uses adaptive min/max buckets. Once it reaches 4,096 entries, adjacent buckets are combined and the bucket width doubles. This bounds its memory independently of source duration. This is a cache-resolution policy, not an audio-duration or file-size restriction.

The polyphonic path first streams the waveform, then analyzes 20-second core regions with 0.5 seconds of context on each available side. Decoded PCM and activation matrices are retained for one region at a time. There is no fixed total-duration or upload-byte limit. The decoder, address space, available storage, and processing time still impose practical resource limits.

## Native polyphonic inference

The model is Spotify's official `basic_pitch/saved_models/icassp_2022/nmp.onnx` from commit `fa5997af0a8210982619003269994a1be25eddf3`. The model is 230,444 bytes and its SHA-256 is `2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec`. Model source URLs, hashes and license are recorded in [PROVENANCE.json](../assets/models/basic-pitch/PROVENANCE.json); the model and adapted note decoder use the included [Apache-2.0 license](../assets/models/basic-pitch/LICENSE).

The runtime dependency is pinned to `ort =2.0.0-rc.10`, targeting ONNX Runtime 1.22.0. Its published arm64 macOS archive checksum was verified against the [ort distribution manifest](https://github.com/pykeio/ort/blob/v2.0.0-rc.10/ort-sys/dist.txt). The archive contains `libonnxruntime.a`. ONNX Runtime’s MIT license and third-party notices are included beside the model; their exact distribution-source provenance is recorded in the manifest. `otool -L` on the native benchmark binary showed only Apple/system libraries and frameworks, with no external ONNX Runtime dylib.

Each neural input is mono/22,050 Hz, produced by the native box resampler. Input windows have 43,844 samples and a 36,164-sample hop. The first input has 3,840 leading zeros. Fifteen output frames are removed from each overlap side. The implementation reads the official ONNX outputs `StatefulPartitionedCall:1` for notes and `:2` for onsets. The note decoder uses onset 0.5, frame 0.3, a five-frame minimum, inferred onsets and the remaining-energy fallback. Model frame times follow the official alignment-offset formula. Candidate amplitudes are model activations, not calibrated probabilities of correct transcription.

The note-decoder algorithm is adapted from the [official Spotify source](https://github.com/spotify/basic-pitch/blob/fa5997af0a8210982619003269994a1be25eddf3/basic_pitch/note_creation.py). Only each region's core survives trimming. Adjacent same-pitch notes are joined across a region boundary only when both predictions independently extend across that boundary. All surviving candidates are preserved. Register and high/low selection define a provisional study line; explicit review remains required before scoring.

The historical TensorFlow.js model, decoder and benchmark remain under `Research/web-v0.2`. Their polyphonic precision/recall values are not a measured accuracy claim for this native ONNX implementation.

## Scoring and attempt boundaries

Scoring uses 20 ms duration bins. The denominator comes from target-voiced time advanced by the transport, including missing microphone frames. Target silence is excluded. A pitch match must fall within the configured tolerance in cents; a different octave is an error. Coverage measures detected singing over target-voiced time, independently of pitch accuracy. Repeated capture updates replace a bin instead of double counting it; sparse capture intervals fill at most 60 ms.

The capture tail helper reserves half an analysis window, positive manual compensation and an 80 ms processing margin. For the current 4,096-sample capture window, call `capture_evaluation_delay(4096, actual_input_sample_rate, compensation_ms)`. Evaluate only up to `evaluated_position(transport_position, attempt_start, delay)`. The audio engine's capture timestamps are already centred and compensated. This margin is a scoring policy, not a physical round-trip latency measurement.

Start a segment on playback/resume and start a fresh attempt on seek/restart/loop as defined by the controller. `begin_segment` prevents a pause or seek gap from being backfilled as missed singing. Finalization should preserve the unevaluated capture tail rather than invent missing samples. The scorer stores the attempt start timestamp and returns an immutable take snapshot for project history.

## Native validation

Analysis tests cover controlled tones and silence, resampling across arbitrary partitions, bounded waveform storage, register selection without bypassing review, cancellation before I/O, native ONNX inference for A4, and preservation of multiple expected notes in a synthetic chord. Scoring tests cover exact pitch versus octave errors, target silence, resume gaps, sparse capture updates, tail policy and persistent take snapshots.

Two additional ignored tests use a private `VOCADITO_CLIP` path. Both were run with `vocadito_1.wav`: the mono pipeline produced 1,329 pitch frames and 73 segmented notes; the polyphonic pipeline crossed two 20-second core regions, produced 106 candidate notes and retained `reviewed=false`. This is a real-recording pipeline smoke test, not a measured polyphonic accuracy benchmark.

```sh
cargo test --no-default-features --lib analysis::tests
cargo test --no-default-features --lib scoring::tests
VOCADITO_CLIP=/private/path/vocadito_1.wav cargo test --no-default-features --lib real_vocadito_clip_smoke -- --ignored --nocapture
VOCADITO_CLIP=/private/path/vocadito_1.wav cargo test --no-default-features --lib real_polyphonic_pipeline_smoke -- --ignored --nocapture
```

## Reproducible native corpus benchmark

The headless [analysis-benchmark](../src/bin/analysis-benchmark.rs) executable directly calls the active Rust decoder, streaming resampler and detector. It requires no Python, Node, FFmpeg, browser, or audio device. It evaluates all reference timestamps against the nearest estimated frame without timing-shift optimization or reference semitone quantization. Unvoiced predictions count as errors on voiced references. Raw pitch accuracy uses ±50 cents; raw chroma accuracy alone permits octave equivalence.

The corpus is [Vocadito, Zenodo 5578807 v3](https://zenodo.org/records/5578807), by Rachel M. Bittner, Katherine Pasalo, Juan José Bosch, Gabriel Meseguer-Brocal and David Rubinstein, licensed CC BY 4.0. Its [technical report](https://arxiv.org/abs/2110.05580) describes the monophonic vocal excerpts and musician-created F0 annotations. The privately cached archive's verified MD5 is `dea40fd18f14d899643c4ba221b33a46`; its SHA-256 is `e0d6b99d3f9c594afe5ae5c4d7bdacebe569e53b809e90b89d1c771c4f9990e3`. No recordings or full annotation CSVs are distributed with the app.

```sh
cargo run --no-default-features --bin analysis-benchmark -- --dataset /private/path/vocadito --limit 40 --output /private/path/native-results.json
```

`--limit 1` is an explicit one-clip smoke subset. Forty clips are required for the full-corpus label. The result records counts, scope, parameters, per-clip metrics and source/annotation FNV-1a fingerprints. Those fingerprints support reproducibility and are not cryptographic integrity guarantees. The archived dataset provenance supplies cryptographic source hashes.

The measured full-corpus result is in [NATIVE_BENCHMARK_RESULTS.json](NATIVE_BENCHMARK_RESULTS.json). This fresh native run measured 40 clips, 140,782 annotated frames: raw pitch accuracy 84.5578%, raw chroma accuracy 85.6926%, voicing recall 91.5657%, false alarm 10.4191%, and 1,055 octave errors. These aggregate counts match the historical monophonic baseline; this does not assert bitwise equivalence of every output frequency.

This corpus evaluates offline monophonic F0. It does not validate live hardware latency, the user's singing, natural choirs, singer identification, neural polyphonic precision, candidate review decisions, or the complete UI/audio/project lifecycle. Controlled-signal tests, real-recording smoke tests and corpus metrics answer different questions and must remain separately reported.
