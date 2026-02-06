//! WAV file audio source for pipe mode.

use crate::audio::recorder::AudioSource;
use crate::defaults::SAMPLE_RATE;
use crate::error::{Result, VoicshError};
use std::io::Read;

/// Audio source that reads from WAV file data.
/// Supports arbitrary sample rates and channels, resampling to 16kHz mono.
pub struct WavAudioSource {
    samples: Vec<i16>,
    position: usize,
    chunk_size: usize,
}

impl WavAudioSource {
    /// Create from any reader (for testing/flexibility).
    pub fn from_reader(reader: Box<dyn Read + Send>) -> Result<Self> {
        let mut wav_reader =
            hound::WavReader::new(reader).map_err(|e| VoicshError::AudioCapture {
                message: format!("Failed to parse WAV file: {}", e),
            })?;

        let spec = wav_reader.spec();
        let source_rate = spec.sample_rate;
        let source_channels = spec.channels;

        // Read all samples from the WAV file
        let raw_samples: Vec<i16> = wav_reader
            .samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| VoicshError::AudioCapture {
                message: format!("Failed to read WAV samples: {}", e),
            })?;

        // Convert to mono if stereo
        let mono_samples = if source_channels == 2 {
            raw_samples
                .chunks_exact(2)
                .map(|chunk| {
                    let left = chunk[0] as i32;
                    let right = chunk[1] as i32;
                    ((left + right) / 2) as i16
                })
                .collect()
        } else {
            raw_samples
        };

        // Resample to 16kHz if needed
        let samples = if source_rate != SAMPLE_RATE {
            resample(&mono_samples, source_rate, SAMPLE_RATE)
        } else {
            mono_samples
        };

        // 100ms chunks at 16kHz
        let chunk_size = 1600;

        Ok(Self {
            samples,
            position: 0,
            chunk_size,
        })
    }

    /// Consume the source and return all samples as a single buffer.
    pub fn into_samples(self) -> Vec<i16> {
        self.samples
    }

    /// Create from stdin.
    pub fn from_stdin() -> Result<Self> {
        use std::io::Cursor;

        // Read all data from stdin into memory first (StdinLock is not Send)
        let mut buffer = Vec::new();
        std::io::stdin()
            .lock()
            .read_to_end(&mut buffer)
            .map_err(|e| VoicshError::AudioCapture {
                message: format!("Failed to read from stdin: {}", e),
            })?;

        Self::from_reader(Box::new(Cursor::new(buffer)))
    }
}

impl AudioSource for WavAudioSource {
    fn start(&mut self) -> Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        Ok(())
    }

    fn read_samples(&mut self) -> Result<Vec<i16>> {
        if self.position >= self.samples.len() {
            return Ok(Vec::new());
        }

        let end = std::cmp::min(self.position + self.chunk_size, self.samples.len());
        let chunk = self.samples[self.position..end].to_vec();
        self.position = end;

        Ok(chunk)
    }
}

/// Simple linear interpolation resampling.
fn resample(samples: &[i16], from_rate: u32, to_rate: u32) -> Vec<i16> {
    if from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio).ceil() as usize;

    (0..output_len)
        .map(|i| {
            let source_pos = i as f64 * ratio;
            let source_idx = source_pos.floor() as usize;
            let fraction = source_pos - source_idx as f64;

            if source_idx + 1 >= samples.len() {
                samples[source_idx]
            } else {
                let left = samples[source_idx] as f64;
                let right = samples[source_idx + 1] as f64;
                (left + (right - left) * fraction) as i16
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_wav_data(sample_rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(&mut cursor, spec).unwrap();
        for &s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
        cursor.into_inner()
    }

    #[test]
    fn from_reader_16khz_mono_matches_exactly() {
        let input_samples = vec![100i16, 200, 300, 400, 500];
        let wav_data = make_wav_data(16000, 1, &input_samples);

        let source = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data))).unwrap();

        assert_eq!(source.samples, input_samples);
        assert_eq!(source.position, 0);
        assert_eq!(source.chunk_size, 1600);
    }

    #[test]
    fn from_reader_16khz_stereo_downmixes_to_mono() {
        // Stereo pairs: (100, 200), (300, 400), (500, 600)
        let stereo_samples = vec![100i16, 200, 300, 400, 500, 600];
        let wav_data = make_wav_data(16000, 2, &stereo_samples);

        let source = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data))).unwrap();

        // Expected mono: (100+200)/2=150, (300+400)/2=350, (500+600)/2=550
        assert_eq!(source.samples, vec![150i16, 350, 550]);
    }

    #[test]
    fn from_reader_48khz_mono_resamples_to_16khz() {
        // 48kHz input: 3 samples for each 16kHz sample
        let input_samples = vec![0i16; 48000]; // 1 second at 48kHz
        let wav_data = make_wav_data(48000, 1, &input_samples);

        let source = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data))).unwrap();

        // Should be resampled to ~16000 samples
        assert!(source.samples.len() >= 15900 && source.samples.len() <= 16100);
    }

    #[test]
    fn from_reader_44100hz_mono_resamples_correctly() {
        let input_samples = vec![1000i16; 44100]; // 1 second at 44.1kHz
        let wav_data = make_wav_data(44100, 1, &input_samples);

        let source = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data))).unwrap();

        // Should be resampled to ~16000 samples
        assert!(source.samples.len() >= 15900 && source.samples.len() <= 16100);
        // Values should be close to original
        assert!(source.samples.iter().all(|&s| (900..=1100).contains(&s)));
    }

    #[test]
    fn read_samples_returns_chunks_of_correct_size() {
        let input_samples = vec![1i16; 5000]; // More than one chunk
        let wav_data = make_wav_data(16000, 1, &input_samples);

        let mut source = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data))).unwrap();

        // First read should return 1600 samples
        let chunk1 = source.read_samples().unwrap();
        assert_eq!(chunk1.len(), 1600);

        // Second read should return another 1600 samples
        let chunk2 = source.read_samples().unwrap();
        assert_eq!(chunk2.len(), 1600);

        // Third read should return another 1600 samples
        let chunk3 = source.read_samples().unwrap();
        assert_eq!(chunk3.len(), 1600);

        // Fourth read should return remaining 200 samples (5000 - 3*1600 = 200)
        let chunk4 = source.read_samples().unwrap();
        assert_eq!(chunk4.len(), 200);
    }

    #[test]
    fn read_samples_returns_empty_vec_at_eof() {
        let input_samples = vec![1i16; 100]; // Small amount
        let wav_data = make_wav_data(16000, 1, &input_samples);

        let mut source = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data))).unwrap();

        // First read returns all samples
        let chunk1 = source.read_samples().unwrap();
        assert_eq!(chunk1.len(), 100);

        // Subsequent reads return empty
        let chunk2 = source.read_samples().unwrap();
        assert_eq!(chunk2.len(), 0);

        let chunk3 = source.read_samples().unwrap();
        assert_eq!(chunk3.len(), 0);
    }

    #[test]
    fn start_stop_are_noops() {
        let input_samples = vec![1i16; 100];
        let wav_data = make_wav_data(16000, 1, &input_samples);

        let mut source = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data))).unwrap();

        // Should not panic or error
        assert!(source.start().is_ok());
        assert!(source.stop().is_ok());
        assert!(source.start().is_ok());
        assert!(source.stop().is_ok());
    }

    #[test]
    fn invalid_wav_data_returns_error() {
        let invalid_data = vec![0u8, 1, 2, 3, 4, 5]; // Not a valid WAV file

        let result = WavAudioSource::from_reader(Box::new(Cursor::new(invalid_data)));

        assert!(result.is_err());
        match result {
            Err(VoicshError::AudioCapture { message }) => {
                assert!(message.contains("Failed to parse WAV file"));
            }
            _ => panic!("Expected AudioCapture error"),
        }
    }

    #[test]
    fn empty_wav_data_returns_error() {
        let empty_data = Vec::new();

        let result = WavAudioSource::from_reader(Box::new(Cursor::new(empty_data)));

        assert!(result.is_err());
    }

    #[test]
    fn resample_identity_same_rate() {
        let samples = vec![100i16, 200, 300, 400, 500];
        let resampled = resample(&samples, 16000, 16000);

        assert_eq!(resampled, samples);
    }

    #[test]
    fn resample_upsample_verification() {
        let samples = vec![0i16, 1000, 2000];
        let resampled = resample(&samples, 8000, 16000);

        // Upsampling from 8kHz to 16kHz should double the sample count
        assert_eq!(resampled.len(), 6);

        // Values should be interpolated
        assert_eq!(resampled[0], 0);
        assert!(resampled[1] > 0 && resampled[1] < 1000);
        assert_eq!(resampled[2], 1000);
    }

    #[test]
    fn resample_downsample_verification() {
        let samples = vec![0i16; 3200]; // 200ms at 16kHz
        let resampled = resample(&samples, 16000, 8000);

        // Downsampling from 16kHz to 8kHz should halve the sample count
        assert_eq!(resampled.len(), 1600);
    }

    #[test]
    fn resample_handles_edge_cases() {
        // Empty input
        let empty = resample(&[], 16000, 8000);
        assert_eq!(empty.len(), 0);

        // Single sample
        let single = resample(&[100i16], 16000, 8000);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0], 100);
    }

    #[test]
    fn read_samples_after_start_stop() {
        let input_samples = vec![1i16, 2, 3, 4, 5];
        let wav_data = make_wav_data(16000, 1, &input_samples);

        let mut source = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data))).unwrap();

        source.start().unwrap();
        let chunk = source.read_samples().unwrap();
        assert_eq!(chunk, input_samples);

        source.stop().unwrap();
        let empty = source.read_samples().unwrap();
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn stereo_downmix_handles_negative_values() {
        // Stereo pairs with negative values: (-100, 100), (300, -300)
        let stereo_samples = vec![-100i16, 100, 300, -300];
        let wav_data = make_wav_data(16000, 2, &stereo_samples);

        let source = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data))).unwrap();

        // Expected: (-100+100)/2=0, (300-300)/2=0
        assert_eq!(source.samples, vec![0i16, 0]);
    }

    #[test]
    fn resample_preserves_signal_amplitude() {
        let samples = vec![1000i16; 100];
        let resampled = resample(&samples, 16000, 8000);

        // All resampled values should be close to 1000
        assert!(resampled.iter().all(|&s| (999..=1001).contains(&s)));
    }

    #[test]
    fn chunk_size_is_100ms_at_16khz() {
        let input_samples = vec![0i16; 100];
        let wav_data = make_wav_data(16000, 1, &input_samples);

        let source = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data))).unwrap();

        // 100ms at 16kHz = 1600 samples
        assert_eq!(source.chunk_size, 1600);
    }

    // Malformed input tests
    #[test]
    fn test_malformed_wav_missing_riff_header() {
        // WAV file without "RIFF" magic bytes
        let bad_data = b"XXXX\x00\x00\x00\x00WAVEfmt ";
        let result = WavAudioSource::from_reader(Box::new(Cursor::new(bad_data.to_vec())));

        assert!(result.is_err(), "Should reject WAV without RIFF header");
        match result {
            Err(VoicshError::AudioCapture { message }) => {
                assert!(
                    message.contains("Failed to parse WAV"),
                    "Error should mention WAV parsing: {}",
                    message
                );
            }
            _ => panic!("Expected AudioCapture error"),
        }
    }

    #[test]
    fn test_malformed_wav_truncated_header() {
        // Truncated WAV header (only first 8 bytes)
        let truncated = b"RIFF\x00\x00";
        let result = WavAudioSource::from_reader(Box::new(Cursor::new(truncated.to_vec())));

        assert!(result.is_err(), "Should reject truncated WAV header");
    }

    #[test]
    fn test_malformed_wav_wrong_format() {
        // RIFF file but not WAVE format
        let wrong_format = b"RIFF\x24\x00\x00\x00XXXX\x00\x00\x00\x00";
        let result = WavAudioSource::from_reader(Box::new(Cursor::new(wrong_format.to_vec())));

        assert!(result.is_err(), "Should reject non-WAVE RIFF files");
    }

    #[test]
    fn test_malformed_wav_missing_fmt_chunk() {
        // RIFF/WAVE but missing fmt chunk
        let no_fmt = b"RIFF\x24\x00\x00\x00WAVEdata\x10\x00\x00\x00\x00\x00\x00\x00";
        let result = WavAudioSource::from_reader(Box::new(Cursor::new(no_fmt.to_vec())));

        assert!(result.is_err(), "Should reject WAV without fmt chunk");
    }

    #[test]
    fn test_malformed_wav_invalid_channel_count() {
        // Create WAV with 0 channels (invalid)
        let mut wav_data = make_wav_data(16000, 1, &vec![0i16; 10]);

        // Find and corrupt the channel count (should be at offset 22)
        // WAV format: RIFF(4) + size(4) + WAVE(4) + fmt(4) + chunksize(4) + format(2) + channels(2)
        // Offset 22 is where channels field starts
        if wav_data.len() > 23 {
            wav_data[22] = 0; // Set channels to 0
            wav_data[23] = 0;

            let result = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data)));
            // Depending on the library, this might be rejected or not
            // Just verify it doesn't panic
            let _ = result;
        }
    }

    #[test]
    fn test_malformed_wav_corrupted_data_chunk() {
        let mut wav_data = make_wav_data(16000, 1, &vec![100i16; 100]);

        // Corrupt the data chunk size to be larger than actual data
        // This should cause issues when trying to read
        if wav_data.len() > 50 {
            // Find "data" chunk and corrupt its size field
            for i in 0..wav_data.len() - 4 {
                if &wav_data[i..i + 4] == b"data" {
                    // Corrupt size to be very large
                    wav_data[i + 4] = 0xFF;
                    wav_data[i + 5] = 0xFF;
                    wav_data[i + 6] = 0xFF;
                    wav_data[i + 7] = 0x7F; // Max i32
                    break;
                }
            }

            let result = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data)));
            // Should either fail to parse or handle gracefully
            if let Ok(mut source) = result {
                // Try to read - should handle gracefully
                let read_result = source.read_samples();
                // Just verify it doesn't panic; might succeed or fail gracefully
                let _ = read_result;
            }
        }
    }

    #[test]
    fn test_malformed_wav_all_zeros() {
        // File is all zeros (clearly not a valid WAV)
        let zeros = vec![0u8; 1000];
        let result = WavAudioSource::from_reader(Box::new(Cursor::new(zeros)));

        assert!(result.is_err(), "Should reject all-zero data");
    }

    #[test]
    fn test_malformed_wav_random_garbage() {
        // Random garbage data (deterministic for reproducibility)
        let mut garbage = Vec::new();
        for i in 0..500 {
            garbage.push(((i * 17 + 42) % 256) as u8); // Pseudo-random but deterministic
        }

        let result = WavAudioSource::from_reader(Box::new(Cursor::new(garbage)));

        assert!(result.is_err(), "Should reject random garbage as WAV");
    }

    #[test]
    fn test_malformed_wav_partial_samples() {
        let mut wav_data = make_wav_data(16000, 1, &vec![100i16; 10]);

        // Truncate the data section to have partial sample (odd number of bytes)
        if wav_data.len() > 20 {
            wav_data.truncate(wav_data.len() - 1); // Remove last byte, creating partial sample

            let result = WavAudioSource::from_reader(Box::new(Cursor::new(wav_data)));
            // Should handle gracefully - either reject or read what's available
            let _ = result;
        }
    }
}
