//! Seekable packet decoding with bounded output blocks. Audio callbacks never use this module.
use crate::model::AudioMetadata;
use anyhow::{Context, Result, bail};
use std::{fs::File, io::ErrorKind, path::Path};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, Decoder, DecoderOptions},
    errors::Error as MediaError,
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    units::{Time, TimeBase},
};

const MAX_READ_FRAMES: usize = 65_536;

pub struct AudioReader {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    time_base: TimeBase,
    metadata: AudioMetadata,
    pending: Vec<f32>,
    pending_frame: usize,
    discard_before: Option<u64>,
    eof: bool,
}

impl AudioReader {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|s| s.to_str()) {
            hint.with_extension(extension);
        }
        let source = MediaSourceStream::new(Box::new(file), Default::default());
        let options = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };
        let probed = symphonia::default::get_probe().format(
            &hint,
            source,
            &options,
            &MetadataOptions::default(),
        )?;
        let format = probed.format;
        let track = format
            .default_track()
            .filter(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .or_else(|| {
                format
                    .tracks()
                    .iter()
                    .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            })
            .context("No supported audio track was found")?;
        let parameters = &track.codec_params;
        let sample_rate = parameters
            .sample_rate
            .context("Audio sample rate is unavailable")?;
        let channels = parameters
            .channels
            .map(|c| c.count() as u16)
            .context("Audio channel layout is unavailable")?;
        if sample_rate == 0 || channels == 0 {
            bail!("Audio must have a positive sample rate and at least one channel");
        }
        let time_base = parameters
            .time_base
            .unwrap_or(TimeBase::new(1, sample_rate));
        let frames = parameters.n_frames.unwrap_or(0);
        let duration_time = time_base.calc_time(frames);
        let metadata = AudioMetadata {
            duration: duration_time.seconds as f64 + duration_time.frac,
            sample_rate,
            channels,
            frames: ((duration_time.seconds as f64 + duration_time.frac) * sample_rate as f64)
                .round() as u64,
            codec: symphonia::default::get_codecs()
                .get_codec(parameters.codec)
                .map_or_else(
                    || format!("{:?}", parameters.codec),
                    |descriptor| descriptor.short_name.to_owned(),
                ),
        };
        let decoder =
            symphonia::default::get_codecs().make(parameters, &DecoderOptions::default())?;
        let track_id = track.id;
        Ok(Self {
            format,
            decoder,
            track_id,
            time_base,
            metadata,
            pending: Vec::new(),
            pending_frame: 0,
            discard_before: None,
            eof: false,
        })
    }

    pub fn metadata(&self) -> AudioMetadata {
        self.metadata.clone()
    }

    /// Accurate demuxer seek followed by packet trimming to the requested native sample frame.
    pub fn seek(&mut self, seconds: f64) -> Result<()> {
        if !seconds.is_finite() || seconds < 0.0 {
            bail!("Seek time must be a finite, nonnegative number");
        }
        let seconds = if self.metadata.duration > 0.0 {
            seconds.min(self.metadata.duration)
        } else {
            seconds
        };
        self.pending.clear();
        self.pending_frame = 0;
        self.decoder.reset();
        self.eof = self.metadata.duration > 0.0 && seconds >= self.metadata.duration;
        self.discard_before = Some((seconds * self.metadata.sample_rate as f64).round() as u64);
        if !self.eof {
            self.format.seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::new(seconds.floor() as u64, seconds.fract()),
                    track_id: Some(self.track_id),
                },
            )?;
        }
        Ok(())
    }

    fn fill_packet(&mut self) -> Result<bool> {
        if self.eof {
            return Ok(false);
        }
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(MediaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                    self.eof = true;
                    return Ok(false);
                }
                Err(error) => return Err(error.into()),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let packet_time = self.time_base.calc_time(packet.ts());
            let packet_start = ((packet_time.seconds as f64 + packet_time.frac)
                * self.metadata.sample_rate as f64)
                .round() as u64;
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(MediaError::DecodeError(_)) => continue,
                Err(error) => return Err(error.into()),
            };
            let spec = *decoded.spec();
            if spec.rate != self.metadata.sample_rate
                || spec.channels.count() != self.metadata.channels as usize
            {
                bail!(
                    "The audio format changes within this file; export it with a constant rate and channel layout"
                );
            }
            let mut converted = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            converted.copy_interleaved_ref(decoded);
            self.pending.clear();
            self.pending.extend_from_slice(converted.samples());
            let frames = self.pending.len() / self.metadata.channels as usize;
            let skipped = self.discard_before.map_or(0, |target| {
                target.saturating_sub(packet_start).min(frames as u64) as usize
            });
            self.pending_frame = skipped;
            if skipped < frames {
                self.discard_before = None;
                return Ok(true);
            }
        }
    }

    pub fn read_mono(&mut self, max_frames: usize) -> Result<Vec<f32>> {
        let limit = max_frames.min(MAX_READ_FRAMES);
        let mut output = Vec::with_capacity(limit);
        let channels = self.metadata.channels as usize;
        while output.len() < limit {
            if self.pending_frame >= self.pending.len() / channels && !self.fill_packet()? {
                break;
            }
            let count =
                (limit - output.len()).min(self.pending.len() / channels - self.pending_frame);
            for frame in self.pending
                [self.pending_frame * channels..(self.pending_frame + count) * channels]
                .chunks_exact(channels)
            {
                output.push(frame.iter().copied().sum::<f32>() / channels as f32);
            }
            self.pending_frame += count;
        }
        Ok(output)
    }

    /// Stereo playback preserves the first two channels; mono sources are duplicated.
    pub fn read_stereo(&mut self, max_frames: usize) -> Result<Vec<[f32; 2]>> {
        let limit = max_frames.min(MAX_READ_FRAMES);
        let mut output = Vec::with_capacity(limit);
        let channels = self.metadata.channels as usize;
        while output.len() < limit {
            if self.pending_frame >= self.pending.len() / channels && !self.fill_packet()? {
                break;
            }
            let count =
                (limit - output.len()).min(self.pending.len() / channels - self.pending_frame);
            for frame in self.pending
                [self.pending_frame * channels..(self.pending_frame + count) * channels]
                .chunks_exact(channels)
            {
                output.push([frame[0], frame[if channels > 1 { 1 } else { 0 }]]);
            }
            self.pending_frame += count;
        }
        Ok(output)
    }
}

/// Files without a container duration are counted through bounded decoding, never decoded in full.
pub fn inspect(path: &Path) -> Result<AudioMetadata> {
    let mut reader = AudioReader::open(path)?;
    let mut metadata = reader.metadata();
    if metadata.frames == 0 {
        let mut frames = 0_u64;
        loop {
            let block = reader.read_mono(16_384)?;
            if block.is_empty() {
                break;
            }
            frames += block.len() as u64;
        }
        metadata.frames = frames;
        metadata.duration = frames as f64 / metadata.sample_rate as f64;
    }
    Ok(metadata)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    pub(crate) fn wav(samples: &[[i16; 2]], rate: u32) -> tempfile::NamedTempFile {
        let mut file = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
        let bytes = (samples.len() * 4) as u32;
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(bytes + 36).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&2_u16.to_le_bytes()).unwrap();
        file.write_all(&rate.to_le_bytes()).unwrap();
        file.write_all(&(rate * 4).to_le_bytes()).unwrap();
        file.write_all(&4_u16.to_le_bytes()).unwrap();
        file.write_all(&16_u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&bytes.to_le_bytes()).unwrap();
        for sample in samples {
            file.write_all(&sample[0].to_le_bytes()).unwrap();
            file.write_all(&sample[1].to_le_bytes()).unwrap();
        }
        file
    }
    #[test]
    fn bounded_blocks_preserve_channels_and_exact_seek() {
        let samples: Vec<_> = (0..10_000).map(|i| [i as i16, -(i as i16)]).collect();
        let file = wav(&samples, 10_000);
        let mut reader = AudioReader::open(file.path()).unwrap();
        assert_eq!(reader.metadata().frames, 10_000);
        assert_eq!(reader.read_stereo(7).unwrap().len(), 7);
        reader.seek(0.5).unwrap();
        let block = reader.read_stereo(3).unwrap();
        assert!((block[0][0] - 5000.0 / 32768.0).abs() < 0.00001);
        assert!((block[0][1] + 5000.0 / 32768.0).abs() < 0.00001);
        reader.seek(0.0).unwrap();
        assert!(
            reader
                .read_mono(19)
                .unwrap()
                .iter()
                .all(|value| value.abs() < 0.00001)
        );
        reader.seek(1.0).unwrap();
        assert!(reader.read_mono(10).unwrap().is_empty());
    }
    #[test]
    fn complete_stream_has_no_duplicates_or_lost_frames() {
        let file = wav(&vec![[4000, 2000]; 80_003], 44_100);
        let mut reader = AudioReader::open(file.path()).unwrap();
        let mut count = 0;
        loop {
            let block = reader.read_mono(127).unwrap();
            if block.is_empty() {
                break;
            }
            assert!(block.len() <= 127);
            count += block.len();
        }
        assert_eq!(count, 80_003);
        assert!(reader.seek(f64::NAN).is_err());
    }
}

#[cfg(test)]
mod album_tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn hour_long_sparse_wav_is_seekable_without_whole_file_decode() {
        let mut file = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
        let frames = 200_000_000_u32;
        let bytes = frames * 4;
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(bytes + 36).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&2_u16.to_le_bytes()).unwrap();
        file.write_all(&48_000_u32.to_le_bytes()).unwrap();
        file.write_all(&192_000_u32.to_le_bytes()).unwrap();
        file.write_all(&4_u16.to_le_bytes()).unwrap();
        file.write_all(&16_u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&bytes.to_le_bytes()).unwrap();
        file.as_file().set_len(bytes as u64 + 44).unwrap();
        let mut reader = AudioReader::open(file.path()).unwrap();
        assert!(reader.metadata().duration > 3600.0);
        assert_eq!(reader.read_mono(17).unwrap().len(), 17);
        reader.seek(3600.0).unwrap();
        assert_eq!(reader.read_stereo(31).unwrap().len(), 31);
        assert!(reader.pending.len() < 131_072);
    }
}
