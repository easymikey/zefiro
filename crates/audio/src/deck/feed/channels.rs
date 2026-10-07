use std::{f32::consts::FRAC_1_SQRT_2, iter};

use symphonia::core::audio::Channels;

pub(crate) fn map_channels(positions: Channels, from: &[f32], to: &mut [f32]) {
    match (from, &mut *to) {
        ([sample], _) => to.fill(*sample),
        (_, [mono]) => {
            let (sum, count) = from.iter().fold((0.0, 0.0), |(sum, count), sample| {
                (sum + sample, count + 1.0)
            });
            *mono = sum / count;
        }
        ([_, _, _, ..], [left, right]) => {
            (*left, *right) = positions.iter().zip(from).fold(
                (0.0, 0.0),
                |(sum_left, sum_right), (channel, sample)| {
                    let (to_left, to_right) = match channel {
                        Channels::FRONT_LEFT => (1.0, 0.0),
                        Channels::FRONT_RIGHT => (0.0, 1.0),
                        Channels::LFE1 | Channels::LFE2 => (0.0, 0.0),
                        Channels::REAR_LEFT
                        | Channels::SIDE_LEFT
                        | Channels::FRONT_LEFT_CENTRE
                        | Channels::REAR_LEFT_CENTRE
                        | Channels::FRONT_LEFT_WIDE
                        | Channels::FRONT_LEFT_HIGH
                        | Channels::TOP_FRONT_LEFT
                        | Channels::TOP_REAR_LEFT => (FRAC_1_SQRT_2, 0.0),
                        Channels::REAR_RIGHT
                        | Channels::SIDE_RIGHT
                        | Channels::FRONT_RIGHT_CENTRE
                        | Channels::REAR_RIGHT_CENTRE
                        | Channels::FRONT_RIGHT_WIDE
                        | Channels::FRONT_RIGHT_HIGH
                        | Channels::TOP_FRONT_RIGHT
                        | Channels::TOP_REAR_RIGHT => (0.0, FRAC_1_SQRT_2),
                        _ => (FRAC_1_SQRT_2, FRAC_1_SQRT_2),
                    };
                    (
                        to_left.mul_add(*sample, sum_left),
                        to_right.mul_add(*sample, sum_right),
                    )
                },
            );
        }
        _ => {
            let samples = from.iter().chain(iter::repeat(&0.0));
            for (slot, sample) in to.iter_mut().zip(samples) {
                *slot = *sample;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{f32::consts::FRAC_1_SQRT_2, io::Write};

    use kernel::domain::revision::Revision;
    use symphonia::core::audio::Channels;

    use crate::deck::{
        feed::{channels::map_channels, feed_channel},
        source::{DecodedTrack, decode},
    };

    #[test]
    fn a_surround_frame_on_a_stereo_device_folds_centre_and_surrounds_and_drops_the_lfe()
     {
        let mut stereo = [0.0; 2];
        let positions = Channels::FRONT_LEFT
            | Channels::FRONT_RIGHT
            | Channels::FRONT_CENTRE
            | Channels::LFE1
            | Channels::REAR_LEFT
            | Channels::REAR_RIGHT;
        map_channels(positions, &[0.1, 0.2, 0.4, 0.8, 0.3, 0.5], &mut stereo);
        let [left, right] = stereo;
        assert!((left - FRAC_1_SQRT_2.mul_add(0.7, 0.1)).abs() < 1e-6);
        assert!((right - FRAC_1_SQRT_2.mul_add(0.9, 0.2)).abs() < 1e-6);
    }

    #[test]
    fn a_stereo_frame_on_a_surround_device_fills_the_other_channels_with_silence() {
        let mut surround = [1.0; 6];
        let positions = Channels::FRONT_LEFT | Channels::FRONT_RIGHT;
        map_channels(positions, &[0.1, 0.2], &mut surround);
        assert_eq!(surround, [0.1, 0.2, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_quad_file_on_a_stereo_device_folds_each_back_channel_into_its_own_side() {
        let quad = [3_277_i16, 6_554, 13_107, 26_214];
        let frames = 100_u32;
        let data_len = frames * 8;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(60 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&40_u32.to_le_bytes());
        bytes.extend_from_slice(&0xFFFE_u16.to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&8_000_u32.to_le_bytes());
        bytes.extend_from_slice(&64_000_u32.to_le_bytes());
        bytes.extend_from_slice(&8_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(&22_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(&0x33_u32.to_le_bytes());
        bytes.extend_from_slice(&[
            0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA,
            0x00, 0x38, 0x9B, 0x71,
        ]);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for _ in 0..frames {
            for value in quad {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let mut file = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
        file.write_all(&bytes).unwrap();
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let decoded_track = DecodedTrack {
            revision: Revision::default(),
            decoder: decode(file.path()).unwrap(),
        };
        let (mut source, mut feed) = feed_channel(decoded_track, 2, callback_sender);
        feed.prime();

        let mut stereo = vec![0.0; 200];
        let read = source.read(&mut stereo);

        let [front_left, front_right, back_left, back_right] =
            quad.map(|value| f32::from(value) / 32_768.0);
        let left = FRAC_1_SQRT_2.mul_add(back_left, front_left);
        let right = FRAC_1_SQRT_2.mul_add(back_right, front_right);
        assert_eq!(read, 200);
        assert!(
            stereo
                .as_chunks::<2>()
                .0
                .iter()
                .all(|[frame_left, frame_right]| {
                    (frame_left - left).abs() < 1e-4
                        && (frame_right - right).abs() < 1e-4
                }),
            "{:?} against {left} {right}",
            &stereo[..2]
        );
    }
}
