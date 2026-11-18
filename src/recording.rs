//! Bounded microphone recording and streaming mono PCM32 WAV/RF64 storage.
//! The hardware callback only enters a producer epoch and pushes finite samples.
use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, TryRecvError, bounded};
use crossbeam_queue::ArrayQueue;
use std::{
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

const RECORDING_CAPACITY: usize = 262_144;
const HEADER_BYTES: u64 = 80;
const OVERFLOW: u32 = 1;
const INPUT_LOST: u32 = 2;
const DISK_ERROR: u32 = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct RecordedAudioSummary {
    pub path: PathBuf,
    pub sample_rate: u32,
    pub frames: u64,
    pub duration: f64,
}

/// Stopping and polling never join the disk writer or perform file I/O.
pub struct RecordingSession {
    bus: Arc<RecordingBus>,
    generation: u64,
    completion: Receiver<Result<RecordedAudioSummary>>,
    consumed: AtomicBool,
}
impl RecordingSession {
    pub fn stop(&self) {
        if self.bus.generation.load(Ordering::Acquire) == self.generation {
            self.bus.stop();
        }
    }
    pub fn is_recording(&self) -> bool {
        self.bus.generation.load(Ordering::Acquire) == self.generation
            && self.bus.active.load(Ordering::Acquire)
    }
    pub fn try_finish(&self) -> Option<Result<RecordedAudioSummary>> {
        if self.consumed.load(Ordering::Acquire) {
            return None;
        }
        match self.completion.try_recv() {
            Ok(result) => {
                self.consumed.store(true, Ordering::Release);
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.consumed.store(true, Ordering::Release);
                Some(Err(anyhow::anyhow!(
                    "The recording writer exited without a result"
                )))
            }
        }
    }
}
impl Drop for RecordingSession {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One stable queue is retained by the callback; sessions cannot overlap finalization.
pub(crate) struct RecordingBus {
    queue: ArrayQueue<f32>,
    active: AtomicBool,
    busy: AtomicBool,
    producers: AtomicUsize,
    generation: AtomicU64,
    error: AtomicU32,
}
impl RecordingBus {
    pub(crate) fn new() -> Arc<Self> {
        Self::with_capacity(RECORDING_CAPACITY)
    }
    fn with_capacity(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            queue: ArrayQueue::new(capacity),
            active: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            producers: AtomicUsize::new(0),
            generation: AtomicU64::new(0),
            error: AtomicU32::new(0),
        })
    }
    pub(crate) fn start(
        self: &Arc<Self>,
        path: PathBuf,
        sample_rate: u32,
    ) -> Result<RecordingSession> {
        if sample_rate == 0 {
            bail!("Recording requires a positive input sample rate");
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            bail!("The previous recording is still active or finalizing");
        }
        while self.queue.pop().is_some() {}
        self.error.store(0, Ordering::Release);
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let (complete, completion) = bounded(1);
        self.active.store(true, Ordering::Release);
        let writer_bus = self.clone();
        if let Err(error) = thread::Builder::new()
            .name("voice-recording-writer".into())
            .spawn(move || {
                let result = writer_bus.write_file(path, sample_rate);
                writer_bus.active.store(false, Ordering::Release);
                // Any producer which observed the old active epoch must finish before reuse.
                while writer_bus.producers.load(Ordering::Acquire) != 0 {
                    thread::yield_now();
                }
                while writer_bus.queue.pop().is_some() {}
                writer_bus.busy.store(false, Ordering::Release);
                let _ = complete.send(result);
            })
        {
            self.active.store(false, Ordering::Release);
            self.busy.store(false, Ordering::Release);
            return Err(error).context("Cannot start the recording writer");
        }
        Ok(RecordingSession {
            bus: self.clone(),
            generation,
            completion,
            consumed: AtomicBool::new(false),
        })
    }
    pub(crate) fn stop(&self) {
        self.active.store(false, Ordering::Release);
    }
    pub(crate) fn fail_input(&self) {
        if self.active.load(Ordering::Acquire) {
            let _ = self
                .error
                .compare_exchange(0, INPUT_LOST, Ordering::AcqRel, Ordering::Acquire);
            self.stop();
        }
    }
    pub(crate) fn enter(&self) -> Option<RecordingTap<'_>> {
        let generation = self.generation.load(Ordering::Acquire);
        if !self.active.load(Ordering::Acquire) {
            return None;
        }
        self.producers.fetch_add(1, Ordering::AcqRel);
        if !self.active.load(Ordering::Acquire)
            || generation != self.generation.load(Ordering::Acquire)
        {
            self.producers.fetch_sub(1, Ordering::Release);
            return None;
        }
        Some(RecordingTap { bus: self })
    }
    fn write_file(&self, path: PathBuf, sample_rate: u32) -> Result<RecordedAudioSummary> {
        let result = (|| -> Result<RecordedAudioSummary> {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .with_context(|| format!("Cannot create recording {}", path.display()))?;
            let mut writer = PcmWriter::new(BufWriter::with_capacity(65_536, file), sample_rate)?;
            let mut block = [0.0_f32; 2048];
            loop {
                let mut count = 0;
                while count < block.len() {
                    let Some(value) = self.queue.pop() else {
                        break;
                    };
                    block[count] = value;
                    count += 1;
                }
                if count != 0 {
                    writer.write_samples(&block[..count])?;
                }
                if !self.active.load(Ordering::Acquire)
                    && self.producers.load(Ordering::Acquire) == 0
                    && self.queue.is_empty()
                {
                    break;
                }
                if count == 0 {
                    thread::sleep(Duration::from_millis(2));
                }
            }
            let frames = writer.frames;
            let mut file = writer.finish()?;
            file.flush()?;
            file.get_ref().sync_all()?;
            match self.error.load(Ordering::Acquire) {
                OVERFLOW => bail!(
                    "Recording failed: the disk writer could not keep up; input samples were lost"
                ),
                INPUT_LOST => bail!("Recording failed: the microphone stream was interrupted"),
                0 => {}
                _ => bail!("Recording failed because of a disk write error"),
            }
            Ok(RecordedAudioSummary {
                path,
                sample_rate,
                frames,
                duration: frames as f64 / sample_rate as f64,
            })
        })();
        if result.is_err() {
            self.error
                .compare_exchange(0, DISK_ERROR, Ordering::AcqRel, Ordering::Acquire)
                .ok();
            self.stop();
        }
        result
    }
}
pub(crate) struct RecordingTap<'a> {
    bus: &'a RecordingBus,
}
impl RecordingTap<'_> {
    /// No allocation, locks, file I/O, or blocking operations occur on this path.
    pub(crate) fn push(&self, sample: f32) {
        if !self.bus.active.load(Ordering::Acquire) {
            return;
        }
        if self
            .bus
            .queue
            .push(if sample.is_finite() { sample } else { 0.0 })
            .is_err()
        {
            self.bus
                .error
                .compare_exchange(0, OVERFLOW, Ordering::AcqRel, Ordering::Acquire)
                .ok();
            self.bus.active.store(false, Ordering::Release);
        }
    }
}
impl Drop for RecordingTap<'_> {
    fn drop(&mut self) {
        self.bus.producers.fetch_sub(1, Ordering::Release);
    }
}

fn pcm32(value: f32) -> i32 {
    if !value.is_finite() {
        0
    } else {
        (value.clamp(-1.0, 1.0) as f64 * 2_147_483_648.0)
            .round()
            .clamp(i32::MIN as f64, i32::MAX as f64) as i32
    }
}
fn header(sample_rate: u32, frames: u64) -> Result<[u8; 80]> {
    let data_bytes = frames
        .checked_mul(4)
        .context("Recording size exceeds the RF64 address space")?;
    let riff_bytes = data_bytes
        .checked_add(HEADER_BYTES - 8)
        .context("Recording size exceeds the RF64 address space")?;
    let rf64 = riff_bytes > u32::MAX as u64;
    let mut bytes = [0_u8; 80];
    bytes[0..4].copy_from_slice(if rf64 { b"RF64" } else { b"RIFF" });
    bytes[4..8].copy_from_slice(&(if rf64 { u32::MAX } else { riff_bytes as u32 }).to_le_bytes());
    bytes[8..12].copy_from_slice(b"WAVE");
    bytes[12..16].copy_from_slice(if rf64 { b"ds64" } else { b"JUNK" });
    bytes[16..20].copy_from_slice(&28_u32.to_le_bytes());
    if rf64 {
        bytes[20..28].copy_from_slice(&riff_bytes.to_le_bytes());
        bytes[28..36].copy_from_slice(&data_bytes.to_le_bytes());
        bytes[36..44].copy_from_slice(&frames.to_le_bytes());
    }
    bytes[48..52].copy_from_slice(b"fmt ");
    bytes[52..56].copy_from_slice(&16_u32.to_le_bytes());
    bytes[56..58].copy_from_slice(&1_u16.to_le_bytes());
    bytes[58..60].copy_from_slice(&1_u16.to_le_bytes());
    bytes[60..64].copy_from_slice(&sample_rate.to_le_bytes());
    bytes[64..68].copy_from_slice(
        &sample_rate
            .checked_mul(4)
            .context("Invalid recording sample rate")?
            .to_le_bytes(),
    );
    bytes[68..70].copy_from_slice(&4_u16.to_le_bytes());
    bytes[70..72].copy_from_slice(&32_u16.to_le_bytes());
    bytes[72..76].copy_from_slice(b"data");
    bytes[76..80].copy_from_slice(&(if rf64 { u32::MAX } else { data_bytes as u32 }).to_le_bytes());
    Ok(bytes)
}
struct PcmWriter<W: Write + Seek> {
    inner: W,
    sample_rate: u32,
    frames: u64,
}
impl<W: Write + Seek> PcmWriter<W> {
    fn new(mut inner: W, sample_rate: u32) -> Result<Self> {
        inner.write_all(&header(sample_rate, 0)?)?;
        Ok(Self {
            inner,
            sample_rate,
            frames: 0,
        })
    }
    fn write_samples(&mut self, samples: &[f32]) -> Result<()> {
        let mut bytes = [0_u8; 8192];
        for block in samples.chunks(bytes.len() / 4) {
            for (value, output) in block.iter().zip(bytes.as_chunks_mut::<4>().0.iter_mut()) {
                output.copy_from_slice(&pcm32(*value).to_le_bytes());
            }
            self.inner.write_all(&bytes[..block.len() * 4])?;
            self.frames = self
                .frames
                .checked_add(block.len() as u64)
                .context("Recording frame count overflow")?;
        }
        Ok(())
    }
    fn finish(mut self) -> Result<W> {
        self.inner.seek(SeekFrom::Start(0))?;
        self.inner
            .write_all(&header(self.sample_rate, self.frames)?)?;
        self.inner.flush()?;
        Ok(self.inner)
    }
}

pub fn inspect_recording(path: &Path) -> Result<RecordedAudioSummary> {
    let reader = RecordingReader::open(path)?;
    Ok(RecordedAudioSummary {
        path: path.to_owned(),
        sample_rate: reader.sample_rate,
        frames: reader.frames,
        duration: reader.frames as f64 / reader.sample_rate as f64,
    })
}

/// Reader for the application's mono PCM32 WAV/RF64 files. Memory is bounded per read.
pub(crate) struct RecordingReader {
    file: BufReader<File>,
    pub(crate) sample_rate: u32,
    pub(crate) frames: u64,
    remaining: u64,
}
impl RecordingReader {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let mut file = BufReader::new(
            File::open(path)
                .with_context(|| format!("Cannot open recording {}", path.display()))?,
        );
        let mut top = [0_u8; 12];
        file.read_exact(&mut top)?;
        let rf64 = &top[..4] == b"RF64";
        if (!rf64 && &top[..4] != b"RIFF") || &top[8..] != b"WAVE" {
            bail!("The recording is not a WAV/RF64 file");
        }
        let file_bytes = file.get_ref().metadata()?.len();
        let declared_riff = u32::from_le_bytes(top[4..8].try_into()?);
        if (!rf64 && declared_riff as u64 + 8 != file_bytes) || (rf64 && declared_riff != u32::MAX)
        {
            bail!("The recording container size does not match the file");
        }
        let mut data_size64 = None;
        let mut sample_count64 = None;
        let mut format = None;
        loop {
            let mut chunk = [0_u8; 8];
            file.read_exact(&mut chunk)
                .context("The recording has no complete data chunk")?;
            let size = u32::from_le_bytes(chunk[4..8].try_into()?) as u64;
            let start = file.stream_position()?;
            match &chunk[..4] {
                b"ds64" if size >= 28 => {
                    let mut ds = [0_u8; 28];
                    file.read_exact(&mut ds)?;
                    data_size64 = Some(u64::from_le_bytes(ds[8..16].try_into()?));
                    sample_count64 = Some(u64::from_le_bytes(ds[16..24].try_into()?));
                    if u64::from_le_bytes(ds[..8].try_into()?).checked_add(8) != Some(file_bytes) {
                        bail!("The RF64 container size does not match the file");
                    }
                }
                b"fmt " if size >= 16 => {
                    let mut fmt = [0_u8; 16];
                    file.read_exact(&mut fmt)?;
                    if u16::from_le_bytes(fmt[..2].try_into()?) != 1
                        || u16::from_le_bytes(fmt[2..4].try_into()?) != 1
                        || u16::from_le_bytes(fmt[12..14].try_into()?) != 4
                        || u16::from_le_bytes(fmt[14..16].try_into()?) != 32
                    {
                        bail!("Replay requires the application's mono PCM32 recording format");
                    }
                    let rate = u32::from_le_bytes(fmt[4..8].try_into()?);
                    if rate == 0 {
                        bail!("Recording sample rate must be positive");
                    }
                    if rate.checked_mul(4) != Some(u32::from_le_bytes(fmt[8..12].try_into()?)) {
                        bail!("The recording byte rate is inconsistent with PCM32 mono");
                    }
                    format = Some(rate);
                }
                b"data" => {
                    let sample_rate = format.context("Recording format is missing")?;
                    if rf64 && (size != u32::MAX as u64 || data_size64.is_none()) {
                        bail!("The RF64 data chunk requires its ds64 size");
                    }
                    let bytes = if rf64 && size == u32::MAX as u64 {
                        data_size64.context("The RF64 data size is missing")?
                    } else {
                        size
                    };
                    if bytes % 4 != 0 {
                        bail!("Recording data is not aligned to PCM32 samples");
                    }
                    if rf64 && sample_count64 != Some(bytes / 4) {
                        bail!("The RF64 sample count does not match its data size");
                    }
                    if start
                        .checked_add(bytes)
                        .context("Invalid recording data length")?
                        != file_bytes
                    {
                        bail!("Recording data size does not match the file");
                    }
                    return Ok(Self {
                        file,
                        sample_rate,
                        frames: bytes / 4,
                        remaining: bytes / 4,
                    });
                }
                _ => {}
            }
            let next = start
                .checked_add(size)
                .and_then(|p| p.checked_add(size % 2))
                .context("Invalid recording chunk size")?;
            if next > file.get_ref().metadata()?.len() {
                bail!("Recording chunk is truncated");
            }
            file.seek(SeekFrom::Start(next))?;
        }
    }
    pub(crate) fn read_mono(&mut self, max_frames: usize) -> Result<Vec<f32>> {
        let count = self.remaining.min(max_frames.min(65_536) as u64) as usize;
        let mut bytes = vec![0_u8; count * 4];
        self.file.read_exact(&mut bytes)?;
        self.remaining -= count as u64;
        Ok(bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|sample| i32::from_le_bytes(*sample) as f32 / 2_147_483_648.0)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Cursor, time::Instant};
    fn wait(session: &RecordingSession) -> Result<RecordedAudioSummary> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(result) = session.try_finish() {
                return result;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn selected_samples_round_trip_with_exact_frames_and_pcm() {
        let dir = tempfile::tempdir().unwrap();
        let bus = RecordingBus::new();
        let session = bus.start(dir.path().join("take.wav"), 48_000).unwrap();
        let samples = [-1.0, -0.75, -0.125, 0.0, 0.125, 0.75, 1.0];
        {
            let tap = bus.enter().unwrap();
            for value in samples {
                tap.push(value);
            }
        }
        session.stop();
        let summary = wait(&session).unwrap();
        assert_eq!(summary.frames, samples.len() as u64);
        assert_eq!(summary.sample_rate, 48_000);
        assert_eq!(summary.duration, samples.len() as f64 / 48_000.0);
        let mut reader = RecordingReader::open(&summary.path).unwrap();
        assert_eq!(reader.read_mono(100).unwrap(), samples);
        assert!(reader.read_mono(100).unwrap().is_empty());
        let mut general = crate::media::AudioReader::open(&summary.path).unwrap();
        assert_eq!(general.read_mono(100).unwrap(), samples);
        assert!(session.try_finish().is_none());
    }
    #[test]
    fn overflow_is_an_explicit_failure_and_finalization_waits_for_producer() {
        let dir = tempfile::tempdir().unwrap();
        let bus = RecordingBus::with_capacity(2);
        // Fill the actual callback queue before starting a consumer to make overflow deterministic.
        bus.active.store(true, Ordering::Release);
        {
            let tap = bus.enter().unwrap();
            tap.push(0.25);
            tap.push(-0.5);
            tap.push(0.75);
        }
        assert!(!bus.active.load(Ordering::Acquire));
        assert_eq!(bus.error.load(Ordering::Acquire), OVERFLOW);
        assert!(
            bus.write_file(dir.path().join("overflow.wav"), 48_000)
                .unwrap_err()
                .to_string()
                .contains("samples were lost")
        );
        let session = bus
            .start(dir.path().join("wait-for-callback.wav"), 48_000)
            .unwrap();
        let tap = bus.enter().unwrap();
        tap.push(0.5);
        session.stop();
        assert!(session.try_finish().is_none());
        drop(tap);
        assert_eq!(wait(&session).unwrap().frames, 1);
    }
    #[test]
    fn stop_and_old_session_drop_cannot_stop_a_new_recording() {
        let dir = tempfile::tempdir().unwrap();
        let bus = RecordingBus::new();
        let first = bus.start(dir.path().join("first.wav"), 44_100).unwrap();
        assert!(bus.start(dir.path().join("overlap.wav"), 44_100).is_err());
        first.stop();
        assert_eq!(wait(&first).unwrap().frames, 0);
        let second = bus.start(dir.path().join("second.wav"), 44_100).unwrap();
        drop(first);
        assert!(second.is_recording());
        second.stop();
        wait(&second).unwrap();
    }
    #[test]
    fn disk_failure_and_input_loss_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let bus = RecordingBus::new();
        let bad = bus
            .start(dir.path().join("missing/take.wav"), 44_100)
            .unwrap();
        assert!(
            wait(&bad)
                .unwrap_err()
                .to_string()
                .contains("Cannot create")
        );
        let interrupted = bus
            .start(dir.path().join("interrupted.wav"), 44_100)
            .unwrap();
        bus.fail_input();
        assert!(
            wait(&interrupted)
                .unwrap_err()
                .to_string()
                .contains("microphone stream")
        );
    }
    #[test]
    fn rf64_header_and_sparse_file_cross_the_riff_limit_without_large_memory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.wav");
        let frames = u32::MAX as u64 / 4 + 10;
        let bytes = header(48_000, frames).unwrap();
        assert_eq!(&bytes[..4], b"RF64");
        assert_eq!(&bytes[12..16], b"ds64");
        assert_eq!(
            u64::from_le_bytes(bytes[28..36].try_into().unwrap()),
            frames * 4
        );
        let mut file = File::create(&path).unwrap();
        file.write_all(&bytes).unwrap();
        file.set_len(80 + frames * 4).unwrap();
        drop(file);
        let mut reader = RecordingReader::open(&path).unwrap();
        assert_eq!(reader.frames, frames);
        assert_eq!(reader.read_mono(16).unwrap(), vec![0.0; 16]);
    }
    #[test]
    fn malformed_recording_lengths_byte_rate_and_rf64_counts_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.wav");
        let mut valid = header(48_000, 2).unwrap().to_vec();
        valid.extend_from_slice(&[0; 8]);
        std::fs::write(&path, &valid).unwrap();
        assert_eq!(inspect_recording(&path).unwrap().frames, 2);
        for (offset, byte) in [(4, 1), (64, 0), (76, 12), (68, 2), (60, 0)] {
            let mut changed = valid.clone();
            changed[offset] ^= byte.max(1);
            std::fs::write(&path, changed).unwrap();
            assert!(inspect_recording(&path).is_err(), "offset {offset}");
        }
        let frames = u32::MAX as u64 / 4 + 10;
        let mut large = header(48_000, frames).unwrap();
        large[36] ^= 1;
        let mut file = File::create(&path).unwrap();
        file.write_all(&large).unwrap();
        file.set_len(80 + frames * 4).unwrap();
        drop(file);
        assert!(
            inspect_recording(&path)
                .unwrap_err()
                .to_string()
                .contains("sample count")
        );
    }
    #[test]
    fn write_error_is_not_hidden_by_header_finalization() {
        struct Limited {
            cursor: Cursor<Vec<u8>>,
            remaining: usize,
        }
        impl Write for Limited {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if self.remaining == 0 {
                    return Err(std::io::Error::other("disk full"));
                }
                let count = self.remaining.min(bytes.len());
                self.remaining -= count;
                self.cursor.write(&bytes[..count])
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl Seek for Limited {
            fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
                self.cursor.seek(position)
            }
        }
        let mut writer = PcmWriter::new(
            Limited {
                cursor: Cursor::new(Vec::new()),
                remaining: 84,
            },
            48_000,
        )
        .unwrap();
        assert!(
            writer
                .write_samples(&[0.25, 0.5])
                .unwrap_err()
                .to_string()
                .contains("disk full")
        );
        let mut writer = PcmWriter::new(
            Limited {
                cursor: Cursor::new(Vec::new()),
                remaining: 84,
            },
            48_000,
        )
        .unwrap();
        writer.write_samples(&[0.25]).unwrap();
        assert!(writer.finish().is_err());
    }
}
