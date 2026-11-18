use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub const PROJECT_VERSION: u32 = 1;
pub fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackRole {
    Target,
    Backing,
}
impl TrackRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Target => "Target",
            Self::Backing => "Backing",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisMode {
    #[default]
    Monophonic,
    Polyphonic,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct PitchFrame {
    pub time: f64,
    pub frequency: f32,
    pub midi: f32,
    pub confidence: f32,
    pub rms: f32,
}
impl PitchFrame {
    pub fn voiced(self) -> bool {
        self.frequency > 0.0 && self.confidence >= 0.7 && self.midi.is_finite()
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Note {
    pub start: f64,
    pub end: f64,
    pub midi: f32,
    #[serde(default = "one")]
    pub confidence: f32,
}
fn one() -> f32 {
    1.0
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct WaveformPeak {
    pub time: f64,
    pub min: f32,
    pub max: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AudioMetadata {
    pub duration: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub codec: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinePolicy {
    #[default]
    Highest,
    Lowest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetSelection {
    pub policy: LinePolicy,
    pub min_midi: u8,
    pub max_midi: u8,
}
impl Default for TargetSelection {
    fn default() -> Self {
        Self {
            policy: LinePolicy::Highest,
            min_midi: 36,
            max_midi: 86,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackAnalysis {
    pub version: String,
    pub mode: AnalysisMode,
    pub frames: Vec<PitchFrame>,
    pub notes: Vec<Note>,
    pub waveform: Vec<WaveformPeak>,
    pub selection: TargetSelection,
    pub reviewed: bool,
}
impl Default for TrackAnalysis {
    fn default() -> Self {
        Self {
            version: "native-1".into(),
            mode: AnalysisMode::Monophonic,
            frames: Vec::new(),
            notes: Vec::new(),
            waveform: Vec::new(),
            selection: TargetSelection::default(),
            reviewed: false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaTrack {
    pub id: String,
    pub role: TrackRole,
    pub filename: String,
    pub archive_path: String,
    pub metadata: AudioMetadata,
    pub analysis: Option<TrackAnalysis>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioPreferences {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub input_channel: u16,
    pub buffer_frames: u32,
    pub input_gain: f32,
    pub monitor: bool,
    pub latency_ms: f32,
}
impl Default for AudioPreferences {
    fn default() -> Self {
        Self {
            input_device: None,
            output_device: None,
            input_channel: 0,
            buffer_frames: 256,
            input_gain: 1.0,
            monitor: false,
            latency_ms: 0.0,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StudioSettings {
    pub target_gain: f32,
    pub backing_gain: f32,
    pub target_muted: bool,
    pub backing_muted: bool,
    pub tolerance_cents: f32,
    pub audio: AudioPreferences,
}
impl Default for StudioSettings {
    fn default() -> Self {
        Self {
            target_gain: 0.65,
            backing_gain: 0.75,
            target_muted: false,
            backing_muted: false,
            tolerance_cents: 50.0,
            audio: AudioPreferences::default(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransportSettings {
    pub cursor: f64,
    pub loop_start: f64,
    pub loop_end: f64,
    pub loop_enabled: bool,
    pub visible_seconds: f64,
}
impl Default for TransportSettings {
    fn default() -> Self {
        Self {
            cursor: 0.0,
            loop_start: 0.0,
            loop_end: 10.0,
            loop_enabled: false,
            visible_seconds: 12.0,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PracticeTake {
    pub id: String,
    pub started_at_ms: u64,
    pub target_id: String,
    pub start: f64,
    pub end: f64,
    pub score: f32,
    pub coverage: f32,
    pub evaluated_seconds: f64,
    pub trace: Vec<PitchFrame>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceProject {
    pub format_version: u32,
    pub id: String,
    pub title: String,
    pub created_at_ms: u64,
    pub modified_at_ms: u64,
    pub target: Option<MediaTrack>,
    pub backing: Option<MediaTrack>,
    pub settings: StudioSettings,
    pub transport: TransportSettings,
    pub history: Vec<PracticeTake>,
}
impl Default for VoiceProject {
    fn default() -> Self {
        let now = timestamp_ms();
        Self {
            format_version: PROJECT_VERSION,
            id: new_id(),
            title: "Untitled".into(),
            created_at_ms: now,
            modified_at_ms: now,
            target: None,
            backing: None,
            settings: StudioSettings::default(),
            transport: TransportSettings::default(),
            history: Vec::new(),
        }
    }
}
impl VoiceProject {
    pub fn duration(&self) -> f64 {
        self.target
            .as_ref()
            .map_or(0.0, |t| t.metadata.duration)
            .max(self.backing.as_ref().map_or(0.0, |t| t.metadata.duration))
    }
    pub fn track(&self, role: TrackRole) -> Option<&MediaTrack> {
        match role {
            TrackRole::Target => self.target.as_ref(),
            TrackRole::Backing => self.backing.as_ref(),
        }
    }
    pub fn track_mut(&mut self, role: TrackRole) -> Option<&mut MediaTrack> {
        match role {
            TrackRole::Target => self.target.as_mut(),
            TrackRole::Backing => self.backing.as_mut(),
        }
    }
}
