//! Self-contained project archives with streamed media and atomic replacement.

use crate::model::{
    AudioMetadata, MediaTrack, PROJECT_VERSION, PitchFrame, TrackRole, VoiceProject, timestamp_ms,
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Component, Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MANIFEST: &str = "project.json";
const TIME_EPSILON: f64 = 0.025;

/// Runtime media paths are separate from the portable document. An opened
/// workspace owns its extracted originals until the workspace is dropped.
#[derive(Default)]
pub struct ProjectWorkspace {
    pub document: VoiceProject,
    pub media_paths: HashMap<TrackRole, PathBuf>,
    pub file_path: Option<PathBuf>,
    _extracted: Option<TempDir>,
}

impl ProjectWorkspace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind_media(&mut self, role: TrackRole, path: impl Into<PathBuf>) {
        self.media_paths.insert(role, path.into());
    }

    /// Commit document state only after the new archive has replaced its target.
    pub fn save(&mut self, path: &Path) -> Result<()> {
        let mut snapshot = self.document.clone();
        snapshot.modified_at_ms = timestamp_ms();
        save_project(&snapshot, &self.media_paths, path)?;
        self.document = snapshot;
        self.file_path = Some(path.to_path_buf());
        Ok(())
    }

    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("Open project {}", path.display()))?;
        let mut archive =
            ZipArchive::new(BufReader::new(file)).context("Read project ZIP directory")?;
        let mut entries = HashMap::new();
        for index in 0..archive.len() {
            let entry = archive.by_index(index).context("Read project ZIP entry")?;
            let name = entry.name().to_owned();
            safe_entry_path(&name, entry.is_dir())?;
            ensure!(
                entry.enclosed_name().is_some(),
                "Unsafe archive entry: {name}"
            );
            ensure!(
                !entry.is_symlink(),
                "Symbolic links are not supported: {name}"
            );
            if let Some(mode) = entry.unix_mode() {
                let kind = mode & 0o170000;
                ensure!(
                    kind == 0 || kind == 0o100000 || kind == 0o040000,
                    "Unsupported archive entry type: {name}"
                );
            }
            ensure!(
                entries.insert(name.clone(), index).is_none(),
                "Duplicate archive entry: {name}"
            );
        }

        let manifest_index = *entries
            .get(MANIFEST)
            .context("Project is missing project.json")?;
        // Read the version independently so a future schema is rejected before
        // attempting to interpret it as today's complete document.
        #[derive(Deserialize)]
        struct VersionHeader {
            format_version: u32,
        }
        let version: VersionHeader = serde_json::from_reader(archive.by_index(manifest_index)?)
            .context("Read project format version")?;
        ensure!(
            version.format_version == PROJECT_VERSION,
            "Unsupported project format version: {} (expected {})",
            version.format_version,
            PROJECT_VERSION
        );
        let document: VoiceProject = serde_json::from_reader(archive.by_index(manifest_index)?)
            .context("Read project metadata")?;
        validate_document(&document)?;

        let extracted = tempfile::Builder::new()
            .prefix("voice-project-")
            .tempdir()
            .context("Create project media workspace")?;
        let mut media_paths = HashMap::new();
        for role in [TrackRole::Target, TrackRole::Backing] {
            let Some(track) = document.track(role) else {
                continue;
            };
            let index = *entries
                .get(&track.archive_path)
                .with_context(|| format!("Missing media asset: {}", track.archive_path))?;
            let mut entry = archive.by_index(index)?;
            ensure!(
                !entry.is_dir() && entry.size() > 0,
                "Media asset is empty or is a directory: {}",
                track.archive_path
            );
            let relative = safe_media_path(&track.archive_path)?;
            let destination = extracted.path().join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).context("Create extracted media directory")?;
            }
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)
                .with_context(|| format!("Create extracted media {}", track.filename))?;
            let expected = entry.size();
            let copied = io::copy(&mut entry, &mut output)
                .with_context(|| format!("Extract media {}", track.filename))?;
            ensure!(
                copied == expected,
                "Incomplete media asset: {}",
                track.archive_path
            );
            output.flush()?;
            media_paths.insert(role, destination);
        }
        Ok(Self {
            document,
            media_paths,
            file_path: Some(path.to_path_buf()),
            _extracted: Some(extracted),
        })
    }
}

/// Save an immutable snapshot on a background thread while its workspace (and
/// therefore any extracted media files) remains alive. No audio is decoded or
/// collected in memory here; each original is copied through bounded I/O.
pub fn save_project(
    document: &VoiceProject,
    media_paths: &HashMap<TrackRole, PathBuf>,
    path: &Path,
) -> Result<()> {
    validate_document(document)?;
    ensure!(path.file_name().is_some(), "A project filename is required");
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary =
        NamedTempFile::new_in(parent).context("Create temporary project beside destination")?;
    {
        let mut writer = ZipWriter::new(BufWriter::new(temporary.as_file_mut()));
        let metadata_options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .large_file(true);
        writer
            .start_file(MANIFEST, metadata_options)
            .context("Start project metadata entry")?;
        serde_json::to_writer(&mut writer, document).context("Write project metadata")?;

        let media_options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .large_file(true);
        for role in [TrackRole::Target, TrackRole::Backing] {
            let Some(track) = document.track(role) else {
                continue;
            };
            let source_path = media_paths
                .get(&role)
                .with_context(|| format!("No local media bound for {}", role.label()))?;
            let file = File::open(source_path)
                .with_context(|| format!("Open original media {}", source_path.display()))?;
            let original_size = file.metadata()?.len();
            ensure!(
                file.metadata()?.is_file() && original_size > 0,
                "Original media must be a nonempty regular file: {}",
                source_path.display()
            );
            writer
                .start_file(&track.archive_path, media_options)
                .with_context(|| format!("Start media entry {}", track.archive_path))?;
            let mut source = BufReader::new(file);
            let copied = io::copy(&mut source, &mut writer)
                .with_context(|| format!("Copy original media {}", track.filename))?;
            ensure!(
                copied == original_size,
                "Original media changed size while saving: {}",
                track.filename
            );
        }
        writer
            .finish()
            .context("Finish project archive")?
            .flush()
            .context("Flush project archive")?;
    }
    temporary
        .as_file()
        .sync_all()
        .context("Synchronize project archive")?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .context("Replace project atomically")?;
    // The archive is committed already. Directory synchronization is best effort
    // because some platforms/filesystems do not support syncing directories.
    #[cfg(unix)]
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn safe_entry_path(name: &str, directory: bool) -> Result<PathBuf> {
    ensure!(
        !name.is_empty() && !name.contains('\0') && !name.contains('\\'),
        "Unsafe archive path: {name:?}"
    );
    let stripped = if directory {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    };
    ensure!(!stripped.is_empty(), "Empty archive path");
    for (index, component) in stripped.split('/').enumerate() {
        ensure!(
            !component.is_empty() && component != "." && component != "..",
            "Unsafe archive path: {name}"
        );
        ensure!(
            !(index == 0 && component.as_bytes().get(1) == Some(&b':')),
            "Absolute drive path is not allowed: {name}"
        );
    }
    let path = Path::new(stripped);
    ensure!(
        path.components()
            .all(|part| matches!(part, Component::Normal(_))),
        "Unsafe archive path: {name}"
    );
    Ok(path.to_path_buf())
}

fn safe_media_path(name: &str) -> Result<PathBuf> {
    let path = safe_entry_path(name, false)?;
    ensure!(
        name.starts_with("media/") && path.components().count() >= 2,
        "Media must be stored under media/: {name}"
    );
    Ok(path)
}

fn finite(value: f64, label: &str) -> Result<()> {
    ensure!(value.is_finite(), "{label} must be finite");
    Ok(())
}

fn nonnegative(value: f64, label: &str) -> Result<()> {
    finite(value, label)?;
    ensure!(value >= 0.0, "{label} must not be negative");
    Ok(())
}

fn bounded(value: f64, minimum: f64, maximum: f64, label: &str) -> Result<()> {
    finite(value, label)?;
    ensure!(
        value >= minimum && value <= maximum,
        "{label} must be between {minimum} and {maximum}"
    );
    Ok(())
}

fn time_in_range(value: f64, start: f64, end: f64, label: &str) -> Result<()> {
    nonnegative(value, label)?;
    ensure!(
        value + TIME_EPSILON >= start && value <= end + TIME_EPSILON,
        "{label} is outside its recording time range"
    );
    Ok(())
}

fn validate_frames(frames: &[PitchFrame], start: f64, end: f64) -> Result<()> {
    let mut previous = f64::NEG_INFINITY;
    for frame in frames {
        time_in_range(frame.time, start, end, "Pitch frame time")?;
        ensure!(
            frame.time >= previous,
            "Pitch frames must be ordered by time"
        );
        previous = frame.time;
        nonnegative(frame.frequency as f64, "Pitch frequency")?;
        bounded(frame.midi as f64, 0.0, 127.0, "Pitch MIDI value")?;
        bounded(frame.confidence as f64, 0.0, 1.0, "Pitch confidence")?;
        nonnegative(frame.rms as f64, "Pitch RMS")?;
    }
    Ok(())
}

fn validate_metadata(metadata: &AudioMetadata) -> Result<()> {
    nonnegative(metadata.duration, "Audio duration")?;
    ensure!(
        metadata.duration > 0.0
            && metadata.sample_rate > 0
            && metadata.channels > 0
            && metadata.frames > 0,
        "Audio metadata must describe nonempty audio"
    );
    Ok(())
}

fn validate_track(track: &MediaTrack, role: TrackRole) -> Result<()> {
    ensure!(
        track.role == role,
        "Media role does not match its project slot"
    );
    ensure!(
        !track.id.is_empty() && !track.filename.is_empty(),
        "Media ID and filename are required"
    );
    safe_media_path(&track.archive_path)?;
    validate_metadata(&track.metadata)?;
    if let Some(analysis) = &track.analysis {
        ensure!(
            analysis.selection.min_midi <= analysis.selection.max_midi
                && analysis.selection.max_midi <= 127,
            "Invalid target selection MIDI range"
        );
        validate_frames(&analysis.frames, 0.0, track.metadata.duration)?;
        for note in &analysis.notes {
            time_in_range(note.start, 0.0, track.metadata.duration, "Note start")?;
            time_in_range(note.end, 0.0, track.metadata.duration, "Note end")?;
            ensure!(note.end > note.start, "Notes must have positive duration");
            bounded(note.midi as f64, 0.0, 127.0, "Note MIDI value")?;
            bounded(note.confidence as f64, 0.0, 1.0, "Note confidence")?;
        }
        for peak in &analysis.waveform {
            time_in_range(peak.time, 0.0, track.metadata.duration, "Waveform time")?;
            finite(peak.min as f64, "Waveform minimum")?;
            finite(peak.max as f64, "Waveform maximum")?;
            ensure!(peak.min <= peak.max, "Waveform minimum exceeds maximum");
        }
    }
    Ok(())
}

fn validate_document(document: &VoiceProject) -> Result<()> {
    ensure!(
        document.format_version == PROJECT_VERSION,
        "Unsupported project format version: {} (expected {})",
        document.format_version,
        PROJECT_VERSION
    );
    ensure!(!document.id.is_empty(), "Project ID is required");
    let mut paths = HashSet::new();
    let mut ids = HashSet::new();
    for role in [TrackRole::Target, TrackRole::Backing] {
        if let Some(track) = document.track(role) {
            validate_track(track, role)?;
            ensure!(
                paths.insert(&track.archive_path),
                "Media entries must have distinct paths"
            );
            ensure!(ids.insert(&track.id), "Media tracks must have distinct IDs");
        }
    }
    let settings = &document.settings;
    nonnegative(settings.target_gain as f64, "Target gain")?;
    nonnegative(settings.backing_gain as f64, "Backing gain")?;
    nonnegative(settings.audio.input_gain as f64, "Input gain")?;
    finite(settings.audio.latency_ms as f64, "Latency compensation")?;
    finite(settings.tolerance_cents as f64, "Pitch tolerance")?;
    ensure!(
        settings.tolerance_cents > 0.0,
        "Pitch tolerance must be positive"
    );
    let transport = &document.transport;
    time_in_range(
        transport.cursor,
        0.0,
        document.duration(),
        "Transport cursor",
    )?;
    nonnegative(transport.loop_start, "Loop start")?;
    nonnegative(transport.loop_end, "Loop end")?;
    finite(transport.visible_seconds, "Visible timeline duration")?;
    ensure!(
        transport.visible_seconds > 0.0,
        "Visible timeline duration must be positive"
    );
    if transport.loop_enabled {
        ensure!(
            transport.loop_start < transport.loop_end
                && transport.loop_end <= document.duration() + TIME_EPSILON,
            "Enabled loop is outside the project duration"
        );
    }
    for take in &document.history {
        nonnegative(take.start, "Practice take start")?;
        nonnegative(take.end, "Practice take end")?;
        ensure!(
            take.end >= take.start,
            "Practice take ends before its start"
        );
        bounded(take.score as f64, 0.0, 100.0, "Practice score")?;
        bounded(take.coverage as f64, 0.0, 100.0, "Practice coverage")?;
        bounded(
            take.evaluated_seconds,
            0.0,
            take.end - take.start + TIME_EPSILON,
            "Evaluated practice duration",
        )?;
        validate_frames(&take.trace, take.start, take.end)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AnalysisMode, LinePolicy, Note, PracticeTake, TrackAnalysis, WaveformPeak};
    use std::io::{Read, Seek, SeekFrom};

    fn frame(time: f64) -> PitchFrame {
        PitchFrame {
            time,
            frequency: 440.0,
            midi: 69.0,
            confidence: 0.95,
            rms: 0.2,
        }
    }

    fn track(role: TrackRole, filename: &str) -> MediaTrack {
        MediaTrack {
            id: format!("test-{}", role.label()),
            role,
            filename: filename.into(),
            archive_path: format!("media/{}/{filename}", role.label()),
            metadata: AudioMetadata {
                duration: 2.0,
                sample_rate: 48_000,
                channels: 1,
                frames: 96_000,
                codec: "pcm_s16le".into(),
            },
            analysis: Some(TrackAnalysis {
                version: "test-analysis-1".into(),
                mode: AnalysisMode::Polyphonic,
                frames: vec![frame(0.5), frame(1.0)],
                notes: vec![Note {
                    start: 0.25,
                    end: 1.5,
                    midi: 69.0,
                    confidence: 0.91,
                }],
                waveform: vec![WaveformPeak {
                    time: 0.0,
                    min: -0.4,
                    max: 0.5,
                }],
                selection: crate::model::TargetSelection {
                    policy: LinePolicy::Lowest,
                    min_midi: 36,
                    max_midi: 86,
                },
                reviewed: true,
            }),
        }
    }

    fn fixture() -> (TempDir, ProjectWorkspace) {
        let directory = tempfile::tempdir().unwrap();
        let mut workspace = ProjectWorkspace::new();
        workspace.document.title = "A complete practice session".into();
        workspace.document.target = Some(track(TrackRole::Target, "target.wav"));
        workspace.document.backing = Some(track(TrackRole::Backing, "backing.flac"));
        for (role, bytes) in [
            (TrackRole::Target, b"original target bytes\0\xff".as_slice()),
            (
                TrackRole::Backing,
                b"original backing bytes\0\x80".as_slice(),
            ),
        ] {
            let path = directory
                .path()
                .join(workspace.document.track(role).unwrap().filename.clone());
            fs::write(&path, bytes).unwrap();
            workspace.bind_media(role, path);
        }
        workspace.document.settings.target_gain = 0.4;
        workspace.document.settings.backing_muted = true;
        workspace.document.settings.audio.input_device = Some("Input interface".into());
        workspace.document.settings.audio.output_device = Some("Headphones".into());
        workspace.document.settings.audio.input_channel = 1;
        workspace.document.settings.audio.buffer_frames = 128;
        workspace.document.settings.audio.latency_ms = 72.5;
        workspace.document.settings.audio.input_gain = 1.4;
        workspace.document.settings.audio.monitor = true;
        workspace.document.transport.cursor = 0.75;
        workspace.document.transport.loop_start = 0.25;
        workspace.document.transport.loop_end = 1.75;
        workspace.document.transport.loop_enabled = true;
        workspace.document.history.push(PracticeTake {
            id: "take-1".into(),
            started_at_ms: 1_000,
            target_id: "test-Target".into(),
            start: 0.25,
            end: 1.75,
            score: 87.5,
            coverage: 95.0,
            evaluated_seconds: 1.25,
            trace: vec![frame(0.5), frame(1.0)],
        });
        (directory, workspace)
    }

    fn write_archive(path: &Path, metadata: &[u8], entries: &[(&str, &[u8])]) {
        let mut writer = ZipWriter::new(File::create(path).unwrap());
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .large_file(true);
        writer.start_file(MANIFEST, options).unwrap();
        writer.write_all(metadata).unwrap();
        for (name, bytes) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn round_trip_preserves_originals_metadata_analysis_history_and_preferences() {
        let (directory, mut workspace) = fixture();
        let path = directory.path().join("complete.voice");
        workspace.save(&path).unwrap();
        let expected = serde_json::to_value(&workspace.document).unwrap();
        fs::remove_file(&workspace.media_paths[&TrackRole::Target]).unwrap();
        fs::remove_file(&workspace.media_paths[&TrackRole::Backing]).unwrap();
        let reopened = ProjectWorkspace::open(&path).unwrap();
        assert_eq!(serde_json::to_value(&reopened.document).unwrap(), expected);
        assert_eq!(
            fs::read(&reopened.media_paths[&TrackRole::Target]).unwrap(),
            b"original target bytes\0\xff"
        );
        assert_eq!(
            fs::read(&reopened.media_paths[&TrackRole::Backing]).unwrap(),
            b"original backing bytes\0\x80"
        );
        assert_eq!(reopened.file_path.as_deref(), Some(path.as_path()));
        let mut archive = ZipArchive::new(File::open(path).unwrap()).unwrap();
        assert_eq!(
            archive
                .by_name("media/Target/target.wav")
                .unwrap()
                .compression(),
            CompressionMethod::Stored
        );
    }

    #[test]
    fn analysis_timestamps_round_trip_without_single_ulp_drift() {
        let (directory, mut workspace) = fixture();
        let timestamp = 11.987499999999999_f64;
        let target = workspace.document.target.as_mut().unwrap();
        target.metadata.duration = 12.0;
        target.metadata.frames = 576_000;
        let analysis = target.analysis.as_mut().unwrap();
        analysis.notes[0].end = timestamp;
        analysis.frames[1].time = timestamp;
        let path = directory.path().join("precise-timestamps.voice");
        workspace.save(&path).unwrap();
        let reopened = ProjectWorkspace::open(&path).unwrap();
        let restored = reopened
            .document
            .target
            .as_ref()
            .unwrap()
            .analysis
            .as_ref()
            .unwrap();
        assert_eq!(restored.notes[0].end.to_bits(), timestamp.to_bits());
        assert_eq!(restored.frames[1].time.to_bits(), timestamp.to_bits());
    }

    #[test]
    fn extracted_originals_live_exactly_as_long_as_the_open_workspace() {
        let (directory, mut workspace) = fixture();
        let path = directory.path().join("owned.voice");
        workspace.save(&path).unwrap();
        let reopened = ProjectWorkspace::open(&path).unwrap();
        let media = reopened.media_paths[&TrackRole::Target].clone();
        assert!(media.is_file());
        drop(reopened);
        assert!(!media.exists());
    }

    #[test]
    fn overwrite_replaces_the_complete_archive() {
        let (directory, mut workspace) = fixture();
        let path = directory.path().join("overwrite.voice");
        workspace.save(&path).unwrap();
        workspace.document.title = "Revised session".into();
        fs::write(
            &workspace.media_paths[&TrackRole::Target],
            b"replacement audio",
        )
        .unwrap();
        workspace.save(&path).unwrap();
        let reopened = ProjectWorkspace::open(&path).unwrap();
        assert_eq!(reopened.document.title, "Revised session");
        assert_eq!(
            fs::read(&reopened.media_paths[&TrackRole::Target]).unwrap(),
            b"replacement audio"
        );
    }

    #[test]
    fn failed_save_preserves_destination_and_workspace_commit_state() {
        let (directory, mut workspace) = fixture();
        let path = directory.path().join("atomic.voice");
        workspace.save(&path).unwrap();
        let before = fs::read(&path).unwrap();
        workspace.document.modified_at_ms = 123;
        let prior_path = workspace.file_path.clone();
        workspace.bind_media(TrackRole::Backing, directory.path().join("missing.wav"));
        assert!(workspace.save(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(workspace.document.modified_at_ms, 123);
        assert_eq!(workspace.file_path, prior_path);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 3);
    }

    #[test]
    fn snapshot_save_does_not_mutate_the_document() {
        let (directory, workspace) = fixture();
        let expected = serde_json::to_value(&workspace.document).unwrap();
        let path = directory.path().join("snapshot.voice");
        save_project(&workspace.document, &workspace.media_paths, &path).unwrap();
        assert_eq!(serde_json::to_value(&workspace.document).unwrap(), expected);
        assert_eq!(
            serde_json::to_value(ProjectWorkspace::open(&path).unwrap().document.clone()).unwrap(),
            expected
        );
    }

    #[test]
    fn supports_target_only_and_empty_projects() {
        let (directory, mut workspace) = fixture();
        workspace.document.backing = None;
        let path = directory.path().join("solo.voice");
        workspace.save(&path).unwrap();
        let reopened = ProjectWorkspace::open(&path).unwrap();
        assert!(reopened.document.backing.is_none());
        assert_eq!(reopened.media_paths.len(), 1);
        let mut empty = ProjectWorkspace::new();
        let empty_path = directory.path().join("empty.voice");
        empty.save(&empty_path).unwrap();
        assert!(
            ProjectWorkspace::open(&empty_path)
                .unwrap()
                .media_paths
                .is_empty()
        );
    }

    #[test]
    fn rejects_unsupported_version_before_parsing_the_current_schema() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("future.voice");
        write_archive(
            &path,
            br#"{"format_version":999,"future_payload":true}"#,
            &[],
        );
        let error = ProjectWorkspace::open(&path).err().unwrap().to_string();
        assert!(
            error.contains("Unsupported project format version"),
            "{error}"
        );
    }

    #[test]
    fn rejects_truncated_zip() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("truncated.voice");
        let mut workspace = ProjectWorkspace::new();
        workspace.save(&path).unwrap();
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(file.metadata().unwrap().len() - 10).unwrap();
        assert!(ProjectWorkspace::open(&path).is_err());
    }

    #[test]
    fn rejects_symbolic_link_entries_before_extraction() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("symlink.voice");
        let mut writer = ZipWriter::new(File::create(&path).unwrap());
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file(MANIFEST, options).unwrap();
        serde_json::to_writer(&mut writer, &VoiceProject::default()).unwrap();
        writer
            .add_symlink("media/link.wav", "../../outside.wav", options)
            .unwrap();
        writer.finish().unwrap();
        let error = ProjectWorkspace::open(&path).err().unwrap().to_string();
        assert!(error.contains("Symbolic links"), "{error}");
    }

    #[test]
    fn rejects_corrupted_media_crc() {
        let (directory, mut workspace) = fixture();
        let path = directory.path().join("corrupted.voice");
        workspace.save(&path).unwrap();
        let offset = {
            let mut archive = ZipArchive::new(File::open(&path).unwrap()).unwrap();
            archive
                .by_name("media/Target/target.wav")
                .unwrap()
                .data_start()
        };
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.write_all(b"!").unwrap();
        drop(file);
        assert!(ProjectWorkspace::open(&path).is_err());
    }

    #[test]
    fn original_media_local_headers_enable_zip64() {
        let (directory, mut workspace) = fixture();
        let path = directory.path().join("zip64.voice");
        workspace.save(&path).unwrap();
        let offset = {
            let mut archive = ZipArchive::new(File::open(&path).unwrap()).unwrap();
            archive
                .by_name("media/Target/target.wav")
                .unwrap()
                .header_start()
        };
        let mut file = File::open(&path).unwrap();
        file.seek(SeekFrom::Start(offset)).unwrap();
        let mut header = [0_u8; 30];
        file.read_exact(&mut header).unwrap();
        assert_eq!(&header[..4], b"PK\x03\x04");
        assert!(u16::from_le_bytes([header[4], header[5]]) >= 45);
        assert_eq!(&header[18..26], &[0xff; 8]);
    }

    #[test]
    fn rejects_traversal_and_platform_absolute_archive_entries() {
        let directory = tempfile::tempdir().unwrap();
        let metadata = serde_json::to_vec(&VoiceProject::default()).unwrap();
        for (index, unsafe_path) in [
            "../escaped.wav",
            "/absolute.wav",
            "C:/outside.wav",
            "media/../../outside.wav",
            "media\\outside.wav",
        ]
        .iter()
        .enumerate()
        {
            let path = directory.path().join(format!("unsafe-{index}.voice"));
            write_archive(&path, &metadata, &[(unsafe_path, b"untrusted")]);
            assert!(
                ProjectWorkspace::open(&path).is_err(),
                "Accepted {unsafe_path}"
            );
        }
        assert!(!directory.path().join("escaped.wav").exists());
    }

    #[test]
    fn rejects_external_metadata_references_and_missing_assets() {
        let (directory, mut workspace) = fixture();
        workspace.document.backing = None;
        let path = directory.path().join("missing.voice");
        write_archive(
            &path,
            &serde_json::to_vec(&workspace.document).unwrap(),
            &[],
        );
        assert!(
            ProjectWorkspace::open(&path)
                .err()
                .unwrap()
                .to_string()
                .contains("Missing media asset")
        );
        workspace.document.target.as_mut().unwrap().archive_path = "../outside.wav".into();
        write_archive(
            &path,
            &serde_json::to_vec(&workspace.document).unwrap(),
            &[],
        );
        assert!(ProjectWorkspace::open(&path).is_err());
    }

    #[test]
    fn rejects_duplicate_archive_entries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("duplicate.voice");
        let metadata = serde_json::to_vec(&VoiceProject::default()).unwrap();
        // zip-rs rejects duplicate writes, so duplicate the central directory
        // filename of a second equal-length entry after finishing the archive.
        write_archive(&path, &metadata, &[("project.jsox", b"{}")]);
        let mut bytes = fs::read(&path).unwrap();
        for start in 0..bytes.len().saturating_sub(12) {
            if &bytes[start..start + 12] == b"project.jsox" {
                bytes[start + 11] = b'n';
            }
        }
        fs::write(&path, bytes).unwrap();
        assert!(ProjectWorkspace::open(&path).is_err());
    }

    #[test]
    fn unicode_filenames_and_titles_round_trip() {
        let (directory, mut workspace) = fixture();
        workspace.document.title = "Choir — étude 🎵".into();
        let target = workspace.document.target.as_mut().unwrap();
        target.filename = "声の練習 — α.wav".into();
        target.archive_path = format!("media/{}/{}", target.id, target.filename);
        let path = directory.path().join("étude 🎵.voice");
        workspace.save(&path).unwrap();
        let reopened = ProjectWorkspace::open(&path).unwrap();
        assert_eq!(reopened.document.title, "Choir — étude 🎵");
        assert_eq!(
            reopened.document.target.as_ref().unwrap().filename,
            "声の練習 — α.wav"
        );
        assert!(reopened.media_paths[&TrackRole::Target].is_file());
    }

    #[test]
    fn rejects_nonfinite_values_and_out_of_bounds_times_before_writing() {
        let (directory, mut workspace) = fixture();
        let path = directory.path().join("invalid.voice");
        workspace.document.settings.audio.latency_ms = f32::NAN;
        assert!(workspace.save(&path).is_err());
        assert!(!path.exists());
        workspace.document.settings.audio.latency_ms = 0.0;
        workspace
            .document
            .target
            .as_mut()
            .unwrap()
            .analysis
            .as_mut()
            .unwrap()
            .notes[0]
            .end = 20.0;
        assert!(workspace.save(&path).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn unknown_current_version_metadata_fields_are_ignored() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("extra.voice");
        let mut value = serde_json::to_value(VoiceProject::default()).unwrap();
        value["extra_information"] = serde_json::json!({"future": "ignored"});
        write_archive(&path, &serde_json::to_vec(&value).unwrap(), &[]);
        assert!(ProjectWorkspace::open(&path).is_ok());
    }

    #[test]
    fn streams_original_audio_larger_than_the_old_web_limits() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("long-original.wav");
        let duration = 901_u64;
        let frames = duration * 48_000;
        let data_size = frames * 2;
        let mut source = File::create(&source_path).unwrap();
        source.write_all(b"RIFF").unwrap();
        source
            .write_all(&((data_size + 36) as u32).to_le_bytes())
            .unwrap();
        source.write_all(b"WAVEfmt ").unwrap();
        source.write_all(&16_u32.to_le_bytes()).unwrap();
        source.write_all(&1_u16.to_le_bytes()).unwrap();
        source.write_all(&1_u16.to_le_bytes()).unwrap();
        source.write_all(&48_000_u32.to_le_bytes()).unwrap();
        source.write_all(&96_000_u32.to_le_bytes()).unwrap();
        source.write_all(&2_u16.to_le_bytes()).unwrap();
        source.write_all(&16_u16.to_le_bytes()).unwrap();
        source.write_all(b"data").unwrap();
        source.write_all(&(data_size as u32).to_le_bytes()).unwrap();
        source.set_len(44 + data_size).unwrap();
        source.seek(SeekFrom::End(-2)).unwrap();
        source.write_all(&1234_i16.to_le_bytes()).unwrap();
        drop(source);
        assert!(fs::metadata(&source_path).unwrap().len() > 80 * 1024 * 1024);
        let mut workspace = ProjectWorkspace::new();
        let mut target = track(TrackRole::Target, "long-original.wav");
        target.metadata = AudioMetadata {
            duration: duration as f64,
            sample_rate: 48_000,
            channels: 1,
            frames,
            codec: "pcm_s16le".into(),
        };
        target.analysis = None;
        workspace.document.target = Some(target);
        workspace.bind_media(TrackRole::Target, source_path);
        let path = directory.path().join("long.voice");
        workspace.save(&path).unwrap();
        let reopened = ProjectWorkspace::open(&path).unwrap();
        assert_eq!(
            reopened.document.target.as_ref().unwrap().metadata.duration,
            901.0
        );
        let mut original = File::open(&reopened.media_paths[&TrackRole::Target]).unwrap();
        assert_eq!(original.metadata().unwrap().len(), 44 + data_size);
        let mut header = [0; 4];
        original.read_exact(&mut header).unwrap();
        assert_eq!(&header, b"RIFF");
        original.seek(SeekFrom::End(-2)).unwrap();
        let mut ending = [0; 2];
        original.read_exact(&mut ending).unwrap();
        assert_eq!(i16::from_le_bytes(ending), 1234);
    }
}
