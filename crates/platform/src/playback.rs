// Cross-platform audio playback via rodio.
//
// The audio output stream (cpal) is not Send on every platform, so a
// dedicated worker thread owns it and receives play requests over a channel.
// Sinks are Send + Sync, so callers hold an Arc<Sink> to wait for completion
// or stop playback without touching the worker thread again.
use std::{
    io::Cursor,
    sync::{
        Arc, Mutex,
        mpsc::{Sender, channel},
    },
    thread,
};

use lazy_allrounder_core::error::PortError;
use rodio::{Decoder, Player, stream::DeviceSinkBuilder, stream::MixerDeviceSink};

struct PlayRequest {
    source: Decoder<Cursor<Vec<u8>>>,
    reply: Sender<Result<Arc<Player>, PortError>>,
}

#[derive(Clone)]
pub struct AudioPlayer {
    worker: Sender<PlayRequest>,
    current: Arc<Mutex<Option<Arc<Player>>>>,
}

impl AudioPlayer {
    pub fn new() -> Self {
        let (worker, requests) = channel::<PlayRequest>();

        thread::Builder::new()
            .name("lazy-allrounder-playback".to_owned())
            .spawn(move || {
                // Opened lazily on the first request so constructing a player
                // never fails in environments without an audio device.
                let mut stream: Option<MixerDeviceSink> = None;

                while let Ok(request) = requests.recv() {
                    let outcome = ensure_stream(&mut stream).map(|stream| {
                        let sink = Arc::new(Player::connect_new(stream.mixer()));
                        sink.append(request.source);
                        sink
                    });
                    let _ = request.reply.send(outcome);
                }
            })
            .expect("failed to spawn the audio playback thread");

        Self {
            worker,
            current: Arc::new(Mutex::new(None)),
        }
    }

    /// Decodes and plays the audio bytes, blocking until playback finishes
    /// or `stop` is called from another handle. Any previous playback is
    /// stopped first.
    pub fn play_and_wait(&self, audio: Vec<u8>) -> Result<(), PortError> {
        // Decoding happens before any audio device is touched, so malformed
        // input fails fast even on machines with no sound output.
        let source = Decoder::new(Cursor::new(audio)).map_err(|error| PortError::Other {
            message: format!("failed to decode audio for playback: {error}"),
        })?;

        self.stop();

        let (reply, response) = channel();
        self.worker
            .send(PlayRequest { source, reply })
            .map_err(|_| playback_thread_gone())?;
        let sink = response.recv().map_err(|_| playback_thread_gone())??;

        *self.lock_current() = Some(sink.clone());
        sink.sleep_until_end();
        self.lock_current().take();

        Ok(())
    }

    /// Stops the currently playing audio, if any. Safe to call from any
    /// thread; a blocked `play_and_wait` returns shortly after.
    pub fn stop(&self) {
        if let Some(sink) = self.lock_current().take() {
            sink.stop();
        }
    }

    pub fn is_playing(&self) -> bool {
        self.lock_current()
            .as_ref()
            .is_some_and(|sink| !sink.empty())
    }

    fn lock_current(&self) -> std::sync::MutexGuard<'_, Option<Arc<Player>>> {
        self.current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

fn ensure_stream(stream: &mut Option<MixerDeviceSink>) -> Result<&MixerDeviceSink, PortError> {
    if stream.is_none() {
        let opened = DeviceSinkBuilder::open_default_sink().map_err(|error| PortError::Other {
            message: format!("failed to open the audio output device: {error}"),
        })?;
        *stream = Some(opened);
    }

    Ok(stream.as_ref().expect("stream was just initialized"))
}

fn playback_thread_gone() -> PortError {
    PortError::Other {
        message: "the audio playback thread is no longer running".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_undecodable_audio_without_touching_a_device() {
        let player = AudioPlayer::new();
        let error = player
            .play_and_wait(vec![0x00, 0x01, 0x02, 0x03])
            .expect_err("garbage bytes should fail to decode");

        assert!(error.to_string().contains("failed to decode audio"));
    }

    #[test]
    fn stop_with_nothing_playing_is_a_no_op() {
        let player = AudioPlayer::new();
        player.stop();
        assert!(!player.is_playing());
    }
}
