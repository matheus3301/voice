# Native F0 benchmark

Measured on 2026-11-17 with the active Rust implementation: **all 40 Vocadito clips**, 817.13 seconds of source audio, 140,782 annotated frames. This is a new native run using Symphonia decoding, the native streaming box resampler and Rust YIN; it does not reuse JavaScript predictions.

| Metric | Native full-corpus result |
| --- | ---: |
| Raw pitch accuracy, ±50 cents | 84.5578% |
| Raw chroma accuracy, ±50 cents | 85.6926% |
| Voicing recall | 91.5657% |
| Voicing false alarm | 10.4191% |
| Octave-error frames | 1,055 |

The [actual result JSON](NATIVE_BENCHMARK_RESULTS.json) includes per-clip counts and metrics, detector parameters, corpus scope, elapsed time and source/annotation reproducibility fingerprints. Native source fingerprint: `fd3c47e2c7a0b881` (FNV-1a, not a cryptographic guarantee). The debug-build run took 37.43 seconds on this machine; this is not a performance guarantee for other devices.

## Corpus and method

[Vocadito, Zenodo 5578807 v3](https://zenodo.org/records/5578807), by Rachel M. Bittner, Katherine Pasalo, Juan José Bosch, Gabriel Meseguer-Brocal and David Rubinstein, is licensed **CC BY 4.0**. Its [technical report](https://arxiv.org/abs/2110.05580) describes solo monophonic vocal recordings with musician-created F0 annotations. The privately cached ZIP has verified MD5 `dea40fd18f14d899643c4ba221b33a46` and SHA-256 `e0d6b99d3f9c594afe5ae5c4d7bdacebe569e53b809e90b89d1c771c4f9990e3`. No recordings or full annotation files are distributed here.

The benchmark compares every annotation timestamp inside each WAV's duration with the nearest native predicted frame; ties choose the earlier frame. There is no timing-shift search or reference-frequency semitone quantization. Windows are 1,024 samples at 16 kHz with a 25 ms hop. A reference F0 greater than zero is voiced. Unvoiced estimates count as incorrect on voiced reference frames. Pitch matches require absolute error ≤50 cents; only chroma accuracy permits octave equivalence. Totals pool frame counts across the complete corpus.

The run contains 92,966 voiced and 47,816 unvoiced reference frames; 78,610 pitch matches, 85,125 detected voiced frames and 4,982 false alarms. These aggregate counts match the archived web baseline, but bitwise equality of frame frequencies is not asserted.

## Reproduce

Extract the official corpus outside the repository, retaining its `Audio/` and `Annotations/F0/` directories. Run from the native checkout:

```sh
cargo run --no-default-features --bin analysis-benchmark -- --dataset /private/path/vocadito --limit 40 --output /private/path/native-results.json
```

`--limit 1` is a one-clip smoke subset and is explicitly labeled as such. The command directly calls the application analysis module and requires no audio device, Node, Python, FFmpeg or browser. Native model/runtime prerequisites are built through the pinned Cargo dependencies. Detailed implementation and validation commands are in [NATIVE_ANALYSIS.md](NATIVE_ANALYSIS.md).

## Limits

This measures offline **monophonic F0**, not live pitch capture, physical latency, note segmentation, scoring accuracy, the user's singing, or UI responsiveness with long albums. Unit tests and real-recording pipeline smoke tests are reported separately.

The bundled native ONNX polyphonic path passed controlled A4/chord inference and a real-recording multi-chunk pipeline smoke test. **It has no measured native polyphonic quality metric yet.** The archived TensorFlow.js artificial-mixture benchmark must not be presented as native ONNX precision, recall or F1. A suitable native polyphonic evaluation must exercise the native weights, resampler, post-processor and chunk merging before making that claim.
