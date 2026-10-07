use std::{ffi::OsStr, fs::File, io::ErrorKind, path::Path, time::Duration};

use kernel::domain::{revision::Revision, speed::Speed};
use symphonia::core::{
    audio::{Channels, SampleBuffer, SignalSpec},
    codecs::{CODEC_TYPE_NULL, Decoder, DecoderOptions},
    errors,
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo, Track},
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
    units::TimeBase,
};

use crate::error::{Error, SeekError};

const READ_CAPACITY: usize = 1 << 20;
const DECODE_SKIPS: usize = 16;

pub struct TrackDecoder {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    pub(crate) spec: SignalSpec,
    buffer: Box<SampleBuffer<f32>>,
    offset: usize,
    pub(crate) path: Box<Path>,
}

impl std::fmt::Debug for TrackDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackDecoder")
            .field("track_id", &self.track_id)
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

impl TrackDecoder {
    pub(crate) fn channels(&self) -> u16 {
        u16::try_from(self.spec.channels.count()).unwrap_or(u16::MAX)
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.spec.rate
    }

    pub(crate) fn duration(&self) -> Option<Duration> {
        let frames = self
            .reader
            .tracks()
            .iter()
            .find(|track| track.id == self.track_id)?
            .codec_params
            .n_frames;
        self.time_base
            .zip(frames)
            .map(|(scale, frames)| scale.calc_time(frames).into())
    }

    pub(crate) fn frames(&mut self) -> Result<&[f32], errors::Error> {
        let mut skips = 0;
        while self.offset >= self.buffer.len() {
            let packet = match self.reader.next_packet() {
                Ok(packet) => packet,
                Err(errors::Error::IoError(error))
                    if error.kind() == ErrorKind::UnexpectedEof =>
                {
                    return Ok(&[]);
                }
                Err(errors::Error::ResetRequired) => {
                    let Ok((track, decoder)) = track_codec(self.reader.as_ref()) else {
                        return Ok(&[]);
                    };
                    if track.codec_params.sample_rate != Some(self.spec.rate)
                        || track.codec_params.channels != Some(self.spec.channels)
                    {
                        return Ok(&[]);
                    }
                    self.track_id = track.id;
                    self.time_base = track.codec_params.time_base;
                    self.decoder = decoder;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    self.spec = *decoded.spec();
                    if decoded.frames() * self.spec.channels.count()
                        > self.buffer.capacity()
                    {
                        let frames =
                            u64::try_from(decoded.capacity()).unwrap_or(u64::MAX);
                        *self.buffer = SampleBuffer::new(frames, self.spec);
                    }
                    self.buffer.copy_interleaved_ref(decoded);
                    self.offset = 0;
                }
                Err(errors::Error::DecodeError(_)) if skips < DECODE_SKIPS => {
                    skips += 1;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(self.buffer.samples().get(self.offset..).unwrap_or(&[]))
    }

    pub(crate) fn consume(&mut self, frames: usize) {
        self.offset += frames * self.spec.channels.count();
    }

    pub(crate) fn seek(&mut self, target: Duration) -> Result<Duration, SeekError> {
        let target = self
            .duration()
            .map_or(target, |duration| target.min(duration));
        let sought = self.reader.seek(
            seek_mode(self.time_base),
            SeekTo::Time {
                time: target.into(),
                track_id: Some(self.track_id),
            },
        )?;
        self.decoder.reset();
        self.buffer.clear();
        self.offset = 0;
        let Some(scale) = self.time_base else {
            return Ok(target);
        };
        let required = Duration::from(scale.calc_time(sought.required_ts));
        let actual = Duration::from(scale.calc_time(sought.actual_ts));
        let rate = u128::from(self.spec.rate);
        let second = Duration::from_secs(1).as_nanos();
        let behind = required.saturating_sub(actual).as_nanos() * rate;
        let mut left =
            usize::try_from((behind + second / 2) / second).unwrap_or(usize::MAX);
        while left > 0 {
            let available = self.frames()?.len() / self.spec.channels.count().max(1);
            if available == 0 {
                break;
            }
            let frames = available.min(left);
            self.consume(frames);
            left -= frames;
        }
        let short = u128::try_from(left).unwrap_or(u128::MAX) * second / rate.max(1);
        let short = u64::try_from(short).unwrap_or(u64::MAX);
        Ok(required.saturating_sub(Duration::from_nanos(short)))
    }
}

fn seek_mode(scale: Option<TimeBase>) -> SeekMode {
    match scale {
        Some(_) => SeekMode::Accurate,
        None => SeekMode::Coarse,
    }
}

pub struct DecodedTrack {
    pub(crate) revision: Revision,
    pub(crate) decoder: TrackDecoder,
}

impl DecodedTrack {
    pub(crate) fn duration(&self) -> Option<Duration> {
        self.decoder.duration()
    }
}

impl PartialEq for DecodedTrack {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
    }
}

impl std::fmt::Debug for DecodedTrack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodedTrack")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PreloadMode {
    Gapless,
    Crossfade(Speed),
}

pub(crate) fn decode(path: &Path) -> Result<TrackDecoder, Error> {
    let file = File::open(path).map_err(|source| Error::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let decoding = |source| Error::Decode {
        path: path.to_path_buf(),
        source,
    };
    let stream = MediaSourceStream::new(
        Box::new(file),
        MediaSourceStreamOptions {
            buffer_len: READ_CAPACITY,
        },
    );
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(OsStr::to_str) {
        hint.with_extension(extension);
    }
    let reader = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions {
                enable_gapless: true,
                ..FormatOptions::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(decoding)?
        .format;
    let (track, decoder) = track_codec(reader.as_ref()).map_err(decoding)?;
    let mut track_decoder = TrackDecoder {
        track_id: track.id,
        time_base: track.codec_params.time_base,
        spec: *decoder.last_decoded().spec(),
        reader,
        decoder,
        buffer: Box::new(SampleBuffer::new(
            0,
            SignalSpec::new(0, Channels::FRONT_LEFT),
        )),
        offset: 0,
        path: path.into(),
    };
    track_decoder.frames().map_err(decoding)?;
    Ok(track_decoder)
}

fn track_codec(
    reader: &dyn FormatReader,
) -> Result<(&Track, Box<dyn Decoder>), errors::Error> {
    let track = reader
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(errors::Error::Unsupported(
            "no track with a supported codec",
        ))?;
    let decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())?;
    Ok((track, decoder))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{io::Write, time::Duration};

    use kernel::message::{AudioError, DecodeError};
    use symphonia::core::{formats::SeekMode, units::TimeBase};
    use tempfile::NamedTempFile;

    use crate::{
        deck::source::{decode, seek_mode},
        error::decode_error_of,
    };

    pub(crate) fn ramp_file(channels: u16, frames: usize) -> NamedTempFile {
        let samples = usize::from(channels) * frames;
        let data_len = u32::try_from(samples * 2).unwrap();
        let rate = 8000_u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        bytes.extend_from_slice(&(channels * 2).to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for index in 0..samples {
            let value = i16::try_from(index % 30_000 + 1).unwrap();
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let mut file = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
        file.write_all(&bytes).unwrap();
        file
    }

    pub(crate) fn decoded(file: &NamedTempFile) -> Vec<f32> {
        let mut decoder = decode(file.path()).unwrap();
        let channels = usize::from(decoder.channels());
        let mut samples = Vec::new();
        while let available @ [_, ..] = decoder.frames().unwrap() {
            samples.extend_from_slice(available);
            let frames = available.len() / channels;
            decoder.consume(frames);
        }
        samples
    }

    fn ramp(samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|index| {
                f32::from(i16::try_from(index % 30_000 + 1).unwrap()) / 32_768.0
            })
            .collect()
    }

    #[test]
    fn a_decoder_yields_a_wav_bit_exact_in_whole_packets() {
        let file = ramp_file(2, 20_000);
        let mut decoder = decode(file.path()).unwrap();
        let mut samples = Vec::new();
        let mut packets = 0;
        while let available @ [_, ..] = decoder.frames().unwrap() {
            assert_eq!(available.len() % 2, 0);
            samples.extend_from_slice(available);
            let frames = available.len() / 2;
            decoder.consume(frames);
            packets += 1;
        }
        assert!(packets > 1);
        assert_eq!(samples.len(), 40_000);
        assert!(samples == ramp(40_000));
    }

    #[test]
    fn a_file_that_is_no_audio_answers_unsupported() {
        let mut file = tempfile::Builder::new().suffix(".txt").tempfile().unwrap();
        file.write_all(b"plain words, no audio in them at all")
            .unwrap();
        assert_eq!(
            decode(file.path()).err().map(decode_error_of),
            Some(AudioError::Decode {
                path: file.path().to_path_buf(),
                error: DecodeError::Unsupported,
            })
        );
    }

    #[test]
    fn a_seek_lands_on_the_target_frame() {
        let file = ramp_file(2, 20_000);
        let mut decoder = decode(file.path()).unwrap();
        let position = decoder.seek(Duration::from_millis(1_250)).unwrap();
        assert_eq!(position, Duration::from_millis(1_250));
        let landed = decoder.frames().unwrap().get(..2).map(<[f32]>::to_vec);
        assert_eq!(landed, ramp(20_002).get(20_000..).map(<[f32]>::to_vec));
    }

    #[test]
    fn a_seek_that_lands_before_its_target_decodes_up_to_the_target() {
        let frame = [&[0xFF, 0xFB, 0x90, 0xC0][..], &[0; 413]].concat();
        let mut file = tempfile::Builder::new().suffix(".mp3").tempfile().unwrap();
        file.write_all(&frame.repeat(40)).unwrap();
        let total = decoded(&file).len();
        let mut decoder = decode(file.path()).unwrap();
        let position = decoder.seek(Duration::from_millis(500)).unwrap();
        let mut rest = 0;
        while let available @ [_, ..] = decoder.frames().unwrap() {
            let frames = available.len();
            rest += frames;
            decoder.consume(frames);
        }
        assert_eq!(
            (position, rest),
            (Duration::from_millis(500), total - 22_050)
        );
    }

    #[test]
    fn a_track_without_a_time_base_seeks_coarsely() {
        assert_eq!(
            (seek_mode(None), seek_mode(Some(TimeBase::new(1, 8_000)))),
            (SeekMode::Coarse, SeekMode::Accurate)
        );
    }
}
