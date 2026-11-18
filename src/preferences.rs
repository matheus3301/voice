//! Machine-local audio preferences, independent of the open project.

use crate::model::AudioPreferences;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

const PREFERENCES_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct PreferencesFile {
    format_version: u32,
    audio: AudioPreferences,
}

/// Return the user's application preferences path without creating it.
pub fn default_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("The user home directory is unavailable")?;
    #[cfg(target_os = "macos")]
    let directory = PathBuf::from(home).join("Library/Application Support/Voice");
    #[cfg(not(target_os = "macos"))]
    let directory = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".config"))
        .join("Voice");
    Ok(directory.join("preferences.json"))
}

fn validate(audio: &AudioPreferences) -> Result<()> {
    ensure!(
        audio.input_gain.is_finite() && audio.input_gain >= 0.0,
        "Input gain must be finite and nonnegative"
    );
    ensure!(
        audio.latency_ms.is_finite(),
        "Latency compensation must be finite"
    );
    ensure!(
        audio.buffer_frames > 0,
        "Requested audio buffer must be positive"
    );
    Ok(())
}

/// A missing preferences file starts with defaults. Corrupt/unsupported files
/// return an error so the application can report them without overwriting them.
pub fn load(path: &Path) -> Result<AudioPreferences> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AudioPreferences::default());
        }
        Err(error) => return Err(error).context("Open audio preferences"),
    };
    let mut preferences: PreferencesFile =
        serde_json::from_reader(BufReader::new(file)).context("Read audio preferences")?;
    ensure!(
        preferences.format_version == PREFERENCES_VERSION,
        "Unsupported audio preferences version: {}",
        preferences.format_version
    );
    validate(&preferences.audio)?;
    // Hardware preferences survive launches; audible monitoring never auto-starts.
    preferences.audio.monitor = false;
    Ok(preferences.audio)
}

/// Atomically save preferences at an explicit location. Monitoring is never
/// persisted as enabled, even when it is active for the current session.
pub fn save(path: &Path, audio: &AudioPreferences) -> Result<()> {
    validate(audio)?;
    ensure!(
        path.file_name().is_some(),
        "A preferences filename is required"
    );
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).context("Create audio preferences directory")?;
    let mut audio = audio.clone();
    audio.monitor = false;
    let preferences = PreferencesFile {
        format_version: PREFERENCES_VERSION,
        audio,
    };
    let mut temporary =
        NamedTempFile::new_in(parent).context("Create temporary audio preferences")?;
    {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        serde_json::to_writer_pretty(&mut writer, &preferences)
            .context("Write audio preferences")?;
        writer.write_all(b"\n")?;
        writer.flush().context("Flush audio preferences")?;
    }
    temporary
        .as_file()
        .sync_all()
        .context("Synchronize audio preferences")?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .context("Replace audio preferences atomically")?;
    #[cfg(unix)]
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_preferences_use_safe_defaults_without_creating_files() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("missing/preferences.json");
        let audio = load(&path).unwrap();
        assert!(!audio.monitor);
        assert_eq!(audio.buffer_frames, 256);
        assert!(!path.exists());
    }

    #[test]
    fn round_trip_remembers_hardware_but_never_enables_monitoring() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary
            .path()
            .join("Application Support/Voice/preferences.json");
        let audio = AudioPreferences {
            input_device: Some("Interface — input".into()),
            output_device: Some("Headphones".into()),
            input_channel: 3,
            buffer_frames: 128,
            input_gain: 1.7,
            monitor: true,
            latency_ms: -17.5,
        };
        save(&path, &audio).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.input_device, audio.input_device);
        assert_eq!(loaded.output_device, audio.output_device);
        assert_eq!(loaded.input_channel, audio.input_channel);
        assert_eq!(loaded.buffer_frames, audio.buffer_frames);
        assert_eq!(loaded.input_gain, audio.input_gain);
        assert_eq!(loaded.latency_ms, audio.latency_ms);
        assert!(!loaded.monitor);
        let stored: PreferencesFile = serde_json::from_reader(File::open(path).unwrap()).unwrap();
        assert!(!stored.audio.monitor);
    }

    #[test]
    fn invalid_save_preserves_the_existing_preferences() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("preferences.json");
        save(&path, &AudioPreferences::default()).unwrap();
        let before = fs::read(&path).unwrap();
        for audio in [
            AudioPreferences {
                input_gain: f32::NAN,
                ..Default::default()
            },
            AudioPreferences {
                latency_ms: f32::INFINITY,
                ..Default::default()
            },
            AudioPreferences {
                buffer_frames: 0,
                ..Default::default()
            },
        ] {
            assert!(save(&path, &audio).is_err());
            assert_eq!(fs::read(&path).unwrap(), before);
        }
    }

    #[test]
    fn corrupt_and_future_preferences_are_not_silently_reset() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("preferences.json");
        fs::write(&path, b"not json").unwrap();
        assert!(load(&path).is_err());
        fs::write(&path, br#"{"format_version":99,"audio":{}}"#).unwrap();
        assert!(load(&path).is_err());
        assert!(fs::read_to_string(path).unwrap().contains("99"));
    }

    #[test]
    fn partial_audio_preferences_gain_current_defaults() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("preferences.json");
        fs::write(
            &path,
            br#"{"format_version":1,"audio":{"input_channel":2,"monitor":true}}"#,
        )
        .unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.input_channel, 2);
        assert_eq!(loaded.input_gain, 1.0);
        assert!(!loaded.monitor);
    }
}
