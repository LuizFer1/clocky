//! Sound, always played on a worker thread. The default sound is a short chime synthesized in
//! code, so the binary carries no audio asset. A user file that fails to open or decode falls
//! back to the chime (spec, section 5). Failures only log a warning.
//!
//! rodio 0.23 renamed the API: `DeviceSinkBuilder` opens the device and `Player` queues sounds.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rodio::Source;

/// The chime: three rising notes (frequency in Hz, length in ms).
const CHIME: [(f32, u64); 3] = [(880.0, 140), (1174.66, 140), (1760.0, 320)];
const VOLUME: f32 = 0.2;

/// Total length of the default chime.
pub fn chime_duration() -> Duration {
    CHIME.iter().map(|(_, ms)| Duration::from_millis(*ms)).sum()
}

fn append_chime(player: &rodio::Player) {
    for (freq, ms) in CHIME {
        player.append(
            rodio::source::SineWave::new(freq)
                .take_duration(Duration::from_millis(ms))
                .fade_out(Duration::from_millis(ms))
                .amplify(VOLUME),
        );
    }
}

/// Opens and decodes an audio file (WAV, MP3, OGG, FLAC...).
pub fn open_file(path: &Path) -> anyhow::Result<rodio::Decoder<BufReader<File>>> {
    let file = File::open(path)?;
    Ok(rodio::Decoder::try_from(file)?)
}

/// Plays the default chime.
pub fn play_default() {
    tracing::info!("playing the default sound ({:?})", chime_duration());
    spawn("chime", |player| {
        append_chime(player);
        Ok(())
    });
}

/// Plays a user file, or the default chime when the file cannot be played.
#[expect(dead_code, reason = "custom alarm sounds arrive in M7")]
pub fn play_file(path: PathBuf) {
    spawn("file", move |player| {
        match open_file(&path) {
            Ok(decoder) => player.append(decoder),
            Err(e) => {
                tracing::warn!(
                    "cannot play {}: {e:#}; playing the default sound",
                    path.display()
                );
                append_chime(player);
            }
        }
        Ok(())
    });
}

fn spawn(
    label: &'static str,
    queue: impl FnOnce(&rodio::Player) -> anyhow::Result<()> + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name(format!("sound-{label}"))
        .spawn(move || {
            if let Err(e) = play(queue) {
                tracing::warn!("sound '{label}' failed: {e:#}");
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("failed to start the sound thread: {e}");
    }
}

/// Opens the default output, queues the sounds and blocks until they end. The device sink must
/// outlive the playback: dropping it cuts the audio.
fn play(queue: impl FnOnce(&rodio::Player) -> anyhow::Result<()>) -> anyhow::Result<()> {
    let mut sink = rodio::DeviceSinkBuilder::open_default_sink()?;
    sink.log_on_drop(false);
    let player = rodio::Player::connect_new(sink.mixer());
    queue(&player)?;
    player.sleep_until_end();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A valid 8 kHz mono 16-bit WAV with `samples` silent samples.
    fn wav_bytes(samples: u32) -> Vec<u8> {
        let data_len = samples * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1_u16.to_le_bytes()); // mono
        bytes.extend_from_slice(&8000_u32.to_le_bytes());
        bytes.extend_from_slice(&16000_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.resize(bytes.len() + data_len as usize, 0);
        bytes
    }

    #[test]
    fn chime_lasts_600_ms() {
        assert_eq!(chime_duration(), Duration::from_millis(600));
    }

    #[test]
    fn valid_wav_decodes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("beep.wav");
        std::fs::write(&path, wav_bytes(800)).unwrap();
        let decoder = open_file(&path).unwrap();
        assert_eq!(decoder.channels().get(), 1);
    }

    #[test]
    fn missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(open_file(&dir.path().join("missing.wav")).is_err());
    }

    #[test]
    fn non_audio_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.mp3");
        std::fs::write(&path, "not audio at all").unwrap();
        assert!(open_file(&path).is_err());
    }
}
