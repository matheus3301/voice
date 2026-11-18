//! Reproducible native monophonic F0 evaluation on a privately cached Vocadito corpus.
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};
use voice::{analysis::analyze_file, media::AudioReader, model::AnalysisMode, scoring::pitch_at};

fn argument(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|value| value == name)
        .and_then(|index| args.get(index + 1).cloned())
}
fn fingerprint(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
fn counts() -> [u64; 8] {
    [0; 8]
}
fn metrics(c: &[u64; 8]) -> serde_json::Value {
    let percent = |a: u64, b: u64| {
        if b > 0 {
            Some(a as f64 / b as f64 * 100.0)
        } else {
            None
        }
    };
    serde_json::json!({"raw_pitch_accuracy_50c":percent(c[4],c[1]),"raw_chroma_accuracy_50c":percent(c[5],c[1]),"voicing_recall":percent(c[3],c[1]),"voicing_false_alarm":percent(c[6],c[2]),"octave_errors":c[7]})
}
fn main() -> Result<()> {
    let root = PathBuf::from(argument("--dataset").context(
        "Usage: analysis-benchmark --dataset /private/Vocadito --limit 40 [--output result.json]",
    )?);
    let limit: usize = argument("--limit").unwrap_or_else(|| "1".into()).parse()?;
    let started = Instant::now();
    let mut total = counts();
    let mut clips = Vec::new();
    for id in 1..=40 {
        if clips.len() >= limit {
            break;
        }
        let audio = root.join("Audio").join(format!("vocadito_{id}.wav"));
        if !audio.exists() {
            continue;
        }
        let annotation = root
            .join("Annotations/F0")
            .join(format!("vocadito_{id}_f0.csv"));
        let duration = AudioReader::open(&audio)?.metadata().duration;
        let before = Instant::now();
        let analysis = analyze_file(
            &audio,
            AnalysisMode::Monophonic,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )?;
        let seconds = before.elapsed().as_secs_f64();
        let mut c = counts();
        let text = fs::read_to_string(&annotation)?;
        for row in text.lines() {
            let mut fields = row.trim().split(',');
            let Some(time) = fields.next().and_then(|field| field.parse::<f64>().ok()) else {
                continue;
            };
            let Some(reference) = fields.next().and_then(|field| field.parse::<f32>().ok()) else {
                continue;
            };
            if time < 0.0 || time >= duration {
                continue;
            }
            c[0] += 1;
            let estimate = pitch_at(&analysis.frames, time);
            if reference > 0.0 {
                c[1] += 1;
                if let Some(estimate) = estimate {
                    c[3] += 1;
                    let cents = 1200.0 * (estimate.frequency / reference).log2();
                    let raw = cents.abs() <= 50.0;
                    let chroma = (cents - (cents / 1200.0).round() * 1200.0).abs() <= 50.0;
                    c[4] += u64::from(raw);
                    c[5] += u64::from(chroma);
                    c[7] += u64::from(chroma && !raw);
                }
            } else {
                c[2] += 1;
                c[6] += u64::from(estimate.is_some());
            }
        }
        for i in 0..8 {
            total[i] += c[i];
        }
        let result = serde_json::json!({"id":format!("vocadito_{id}"),"duration_seconds":duration,"analysis_seconds":seconds,"pitch_frames":analysis.frames.len(),"notes":analysis.notes.len(),"waveform_bins":analysis.waveform.len(),"counts":c,"metrics":metrics(&c),"annotation_fnv1a64":fingerprint(text.as_bytes())});
        eprintln!("vocadito_{id}: {}", result["metrics"]);
        clips.push(result);
    }
    anyhow::ensure!(!clips.is_empty(), "No Vocadito WAV/CSV pairs found");
    let result = serde_json::json!({"schema_version":1,"application_version":env!("CARGO_PKG_VERSION"),"analysis_version":"native-yin-1","scope":if clips.len()==40{"entire 40-clip Vocadito corpus"}else{"explicit subset; not the complete corpus"},"clip_count":clips.len(),"dataset":"Vocadito Zenodo 5578807 v3, CC-BY-4.0; see Research/web-v0.2/docs/BENCHMARK.md for attribution and source hashes","source_archive_md5":"dea40fd18f14d899643c4ba221b33a46","analysis_source_fnv1a64":fingerprint(include_bytes!("../analysis.rs")),"fingerprint_note":"FNV-1a is a reproducibility fingerprint, not a cryptographic integrity guarantee","protocol":{"decoder":"native Symphonia, original sample rate/channel count, mono mix","resampler":"native streaming box integration to 16000Hz","window_samples":1024,"hop_seconds":0.025,"reference":"continuous published F0, nearest predicted frame by timestamp, earlier on ties, no time-shift optimization","raw_pitch_tolerance_cents":50,"unvoiced":"reference F0 <=0; unvoiced predictions count as incorrect on voiced reference","count_order":["frames","reference_voiced","reference_unvoiced","detected_voiced","raw_correct","chroma_correct","false_alarm","octave_errors"]},"elapsed_seconds":started.elapsed().as_secs_f64(),"counts":total,"micro_metrics":metrics(&total),"clips":clips});
    let json = serde_json::to_string_pretty(&result)?;
    if let Some(output) = argument("--output") {
        fs::write(Path::new(&output), format!("{json}\n"))?;
    } else {
        println!("{json}");
    }
    Ok(())
}
