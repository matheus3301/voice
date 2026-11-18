//! Headless import, analysis, archive save, and reopen integration smoke check.
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::OsString,
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};
use voice::{
    analysis::analyze_file,
    media,
    model::{AnalysisMode, MediaTrack, TrackRole, new_id},
    project::ProjectWorkspace,
};

const USAGE: &str = "Usage: project-smoke --target TARGET.wav [--backing BACKING.wav] --output SESSION.voice [--polyphonic]";

struct Arguments {
    target: PathBuf,
    backing: Option<PathBuf>,
    output: PathBuf,
    mode: AnalysisMode,
}

fn arguments() -> Result<Option<Arguments>> {
    let mut arguments = std::env::args_os().skip(1);
    let mut target = None;
    let mut backing = None;
    let mut output = None;
    let mut mode = AnalysisMode::Monophonic;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--help" | "-h") => {
                println!(
                    "{USAGE}\n\nRuns without the desktop feature or an audio device. Originals are compared in bounded chunks after reopening the saved project."
                );
                return Ok(None);
            }
            Some("--target") => target = Some(path_argument(&mut arguments, "--target")?),
            Some("--backing") => backing = Some(path_argument(&mut arguments, "--backing")?),
            Some("--output") => output = Some(path_argument(&mut arguments, "--output")?),
            Some("--polyphonic") => mode = AnalysisMode::Polyphonic,
            _ => bail!("Unknown argument: {}\n{USAGE}", argument.to_string_lossy()),
        }
    }
    Ok(Some(Arguments {
        target: target.with_context(|| format!("Missing --target\n{USAGE}"))?,
        backing,
        output: output.with_context(|| format!("Missing --output\n{USAGE}"))?,
        mode,
    }))
}

fn path_argument(arguments: &mut impl Iterator<Item = OsString>, name: &str) -> Result<PathBuf> {
    let value = arguments
        .next()
        .with_context(|| format!("Missing path after {name}"))?;
    ensure!(
        !value.to_string_lossy().starts_with("--"),
        "Missing path after {name}"
    );
    Ok(PathBuf::from(value))
}

fn import_track(
    workspace: &mut ProjectWorkspace,
    path: &Path,
    role: TrackRole,
    mode: AnalysisMode,
) -> Result<()> {
    let metadata =
        media::inspect(path).with_context(|| format!("Inspect {} audio", role.label()))?;
    eprintln!(
        "{}: {:.3} seconds, {} Hz, {} channel(s)",
        role.label(),
        metadata.duration,
        metadata.sample_rate,
        metadata.channels
    );
    let reported = AtomicUsize::new(0);
    let analysis = analyze_file(path, mode, Arc::new(AtomicBool::new(false)), |fraction| {
        let bucket = (fraction.clamp(0.0, 1.0) * 4.0) as usize;
        if bucket > reported.load(Ordering::Relaxed) {
            reported.store(bucket, Ordering::Relaxed);
            eprintln!("{} analysis: {}%", role.label(), bucket * 25);
        }
    })
    .with_context(|| format!("Analyze {} audio", role.label()))?;
    if role == TrackRole::Target {
        ensure!(
            !analysis.notes.is_empty(),
            "Target analysis detected no notes; use a pitched fixture for this integration smoke check"
        );
    }
    let id = new_id();
    let filename = path
        .file_name()
        .context("Audio path has no filename")?
        .to_string_lossy()
        .into_owned();
    let track = MediaTrack {
        archive_path: format!("media/{id}/{filename}"),
        id,
        role,
        filename,
        metadata,
        analysis: Some(analysis),
    };
    match role {
        TrackRole::Target => workspace.document.target = Some(track),
        TrackRole::Backing => workspace.document.backing = Some(track),
    }
    workspace.bind_media(role, path);
    Ok(())
}

/// Compare every original byte without allocating an audio-sized buffer.
fn compare_originals(source: &Path, reopened: &Path) -> Result<u64> {
    let mut source = BufReader::new(File::open(source)?);
    let mut reopened = BufReader::new(File::open(reopened)?);
    let length = source.get_ref().metadata()?.len();
    ensure!(
        length == reopened.get_ref().metadata()?.len(),
        "Original media size changed in the archive"
    );
    let mut remaining = length;
    let mut source_bytes = [0_u8; 64 * 1024];
    let mut reopened_bytes = [0_u8; 64 * 1024];
    while remaining > 0 {
        let count = remaining.min(source_bytes.len() as u64) as usize;
        source.read_exact(&mut source_bytes[..count])?;
        reopened.read_exact(&mut reopened_bytes[..count])?;
        ensure!(
            source_bytes[..count] == reopened_bytes[..count],
            "Original media bytes changed in the archive"
        );
        remaining -= count as u64;
    }
    let mut trailing = [0_u8; 1];
    ensure!(
        source.read(&mut trailing)? == 0 && reopened.read(&mut trailing)? == 0,
        "Original media changed size during verification"
    );
    Ok(length)
}

fn first_difference(
    expected: &serde_json::Value,
    actual: &serde_json::Value,
    path: &str,
) -> Option<String> {
    if expected == actual {
        return None;
    }
    match (expected, actual) {
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            for (key, value) in left {
                match right.get(key) {
                    Some(other) => {
                        if let Some(difference) =
                            first_difference(value, other, &format!("{path}.{key}"))
                        {
                            return Some(difference);
                        }
                    }
                    None => return Some(format!("{path}.{key}: missing after reopening")),
                }
            }
        }
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            if left.len() != right.len() {
                return Some(format!(
                    "{path}: length {} became {}",
                    left.len(),
                    right.len()
                ));
            }
            for (index, (value, other)) in left.iter().zip(right).enumerate() {
                if let Some(difference) =
                    first_difference(value, other, &format!("{path}[{index}]"))
                {
                    return Some(difference);
                }
            }
        }
        _ => {}
    }
    Some(format!("{path}: {expected} became {actual}"))
}

fn main() -> Result<()> {
    let Some(arguments) = arguments()? else {
        return Ok(());
    };
    let started = Instant::now();
    let mut workspace = ProjectWorkspace::new();
    workspace.document.title = arguments
        .target
        .file_stem()
        .context("Target has no filename")?
        .to_string_lossy()
        .into_owned();
    import_track(
        &mut workspace,
        &arguments.target,
        TrackRole::Target,
        arguments.mode,
    )?;
    if let Some(backing) = &arguments.backing {
        import_track(
            &mut workspace,
            backing,
            TrackRole::Backing,
            AnalysisMode::Monophonic,
        )?;
    }
    workspace.document.transport.cursor = (workspace.document.duration() / 3.0).min(7.0);
    workspace
        .save(&arguments.output)
        .context("Save complete project")?;
    let expected = serde_json::to_value(&workspace.document)?;
    let reopened = ProjectWorkspace::open(&arguments.output).context("Reopen saved project")?;
    let actual = serde_json::to_value(&reopened.document)?;
    if let Some(difference) = first_difference(&expected, &actual, "project") {
        bail!("Project metadata or cached analysis changed after reopening: {difference}");
    }

    let mut tracks = Vec::new();
    for role in [TrackRole::Target, TrackRole::Backing] {
        let Some(track) = reopened.document.track(role) else {
            continue;
        };
        let bytes = compare_originals(&workspace.media_paths[&role], &reopened.media_paths[&role])
            .with_context(|| format!("Verify {} original", role.label()))?;
        let cached = track
            .analysis
            .as_ref()
            .context("Cached analysis is missing")?;
        tracks.push(serde_json::json!({
            "role": role,
            "filename": track.filename,
            "original_bytes_verified": bytes,
            "metadata": track.metadata,
            "analysis_version": cached.version,
            "cached_pitch_frames": cached.frames.len(),
            "cached_notes": cached.notes.len(),
            "cached_waveform_peaks": cached.waveform.len(),
        }));
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "status": "passed",
            "scope": "headless import, native analysis, atomic project save, reopen, and original-byte verification",
            "application_version": env!("CARGO_PKG_VERSION"),
            "project_path": arguments.output,
            "format_version": reopened.document.format_version,
            "target_mode": arguments.mode,
            "all_document_metadata_preserved": true,
            "elapsed_seconds": started.elapsed().as_secs_f64(),
            "tracks": tracks,
            "limitations": "This check does not exercise GPUI file panels, microphone capture, playback devices, or physical audio latency.",
        }))?
    );
    Ok(())
}
