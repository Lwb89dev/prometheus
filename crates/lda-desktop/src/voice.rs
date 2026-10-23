//! Push-to-talk capture. Nothing here opens a microphone until [`Capture::start`].
//! There is no wake word.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use lda_common::{wav_is_usable, Error, Result, MAX_AUDIO_BYTES};

pub trait VoiceBackend: Send + Sync {
    fn available(&self) -> bool;
    fn push_to_talk(&self) -> bool;
    fn wake_word(&self) -> bool;
}

/// Explicit off switch. `available` stays false and no program is spawned.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoVoice;

impl VoiceBackend for NoVoice {
    fn available(&self) -> bool {
        false
    }

    fn push_to_talk(&self) -> bool {
        false
    }

    fn wake_word(&self) -> bool {
        false
    }
}

/// Reports a local recorder when `pw-record` or `parecord` is on `PATH`.
/// Looking it up does not open a device.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalVoice;

impl VoiceBackend for LocalVoice {
    fn available(&self) -> bool {
        recorder_name().is_some()
    }

    fn push_to_talk(&self) -> bool {
        recorder_name().is_some()
    }

    fn wake_word(&self) -> bool {
        false
    }
}

/// One recording. Dropping it kills the process and deletes the file.
pub struct Capture {
    child: Child,
    path: PathBuf,
}

impl Capture {
    pub fn start() -> Result<Self> {
        let program = recorder_name().ok_or_else(|| {
            Error::Other(
                "No pw-record or parecord is installed. The microphone stays closed.".into(),
            )
        })?;
        let path = std::env::temp_dir().join(format!("lda-talk-{}.wav", lda_common::new_id()));
        let mut command = Command::new(program);
        command
            .args(recorder_args(program, &path))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        // pw-record writes status to stderr. Nobody reading that pipe lets it
        // fill, and the recorder blocks until Talk is stuck.
        let errors = spawn_stderr_drain(child.stderr.take());
        std::thread::sleep(Duration::from_millis(150));
        if let Some(status) = child.try_wait()? {
            let detail = errors
                .recv_timeout(Duration::from_secs(1))
                .unwrap_or_default();
            let _ = std::fs::remove_file(&path);
            return Err(Error::Other(format!(
                "{program} exited {status} before recording. {detail}"
            )));
        }
        Ok(Self { child, path })
    }

    pub fn stop(mut self) -> Result<Vec<u8>> {
        finish_child(&mut self.child);
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::InvalidInput(
                    "the recording was empty or not a wav. The microphone is closed.".into(),
                ));
            }
            Err(err) => return Err(err.into()),
        };
        if bytes.len() > MAX_AUDIO_BYTES || !wav_is_usable(&bytes) {
            return Err(Error::InvalidInput(
                "the recording was empty or not a wav. The microphone is closed.".into(),
            ));
        }
        Ok(bytes)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn play_wav(bytes: &[u8]) -> Result<()> {
    if !wav_is_usable(bytes) {
        return Err(Error::InvalidInput(
            "the speech model did not return a wav".into(),
        ));
    }
    let program = player_name().ok_or_else(|| {
        Error::Other("No pw-play or paplay is installed, so the wav was not played.".into())
    })?;
    let path = std::env::temp_dir().join(format!("lda-speech-{}.wav", lda_common::new_id()));
    std::fs::write(&path, bytes)?;
    let status = Command::new(program)
        .arg(&path)
        .stdin(Stdio::null())
        .status();
    let _ = std::fs::remove_file(&path);
    let status = status?;
    if status.success() {
        return Ok(());
    }
    Err(Error::Other(format!("{program} exited {status}")))
}

pub fn recorder_name() -> Option<&'static str> {
    if command_on_path("pw-record") {
        return Some("pw-record");
    }
    if command_on_path("parecord") {
        return Some("parecord");
    }
    None
}

pub fn player_name() -> Option<&'static str> {
    if command_on_path("pw-play") {
        return Some("pw-play");
    }
    if command_on_path("paplay") {
        return Some("paplay");
    }
    None
}

fn recorder_args(program: &str, path: &Path) -> Vec<String> {
    let file = path.display().to_string();
    if program == "parecord" {
        return vec![
            "--record".to_string(),
            "--rate=16000".to_string(),
            "--channels=1".to_string(),
            "--format=s16le".to_string(),
            "--file-format=wav".to_string(),
            file,
        ];
    }
    vec![
        "--rate".to_string(),
        "16000".to_string(),
        "--channels".to_string(),
        "1".to_string(),
        "--format".to_string(),
        "s16".to_string(),
        "--container".to_string(),
        "wav".to_string(),
        file,
    ]
}

fn command_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(name).is_file())
}

fn finish_child(child: &mut Child) {
    let _ = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Err(_) => return,
            Ok(None) if start.elapsed() >= Duration::from_secs(2) => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
        }
    }
}

fn spawn_stderr_drain(
    stderr: Option<std::process::ChildStderr>,
) -> std::sync::mpsc::Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(drain_stderr(stderr));
    });
    rx
}

/// Read until the pipe closes. Only the first 400 bytes are kept.
fn drain_stderr(stderr: Option<impl Read>) -> String {
    let Some(mut stderr) = stderr else {
        return String::new();
    };
    let mut kept = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        let read = match stderr.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if kept.len() >= 400 {
            continue;
        }
        let room = 400 - kept.len();
        kept.extend_from_slice(&buf[..read.min(room)]);
    }
    String::from_utf8_lossy(&kept).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_is_off_until_a_recorder_exists() {
        let voice = NoVoice;
        assert!(!voice.available());
        assert!(!voice.push_to_talk());
        assert!(!voice.wake_word());
        let local = LocalVoice;
        assert!(!local.wake_word());
        assert_eq!(local.available(), local.push_to_talk());
    }

    #[test]
    fn recorder_args_name_the_wav() {
        let path = Path::new("/tmp/lda-talk.wav");
        let pipe = recorder_args("pw-record", path);
        assert!(pipe.contains(&"16000".to_string()));
        assert!(pipe.iter().any(|arg| arg.ends_with("lda-talk.wav")));
        let pulse = recorder_args("parecord", path);
        assert!(pulse.iter().any(|arg| arg == "--record"));
        assert!(pulse.iter().any(|arg| arg == "--channels=1"));
        assert!(pulse.iter().any(|arg| arg.ends_with("lda-talk.wav")));
    }

    #[test]
    fn stderr_drain_keeps_reading_after_the_prefix() {
        use std::cell::Cell;
        use std::rc::Rc;

        let hits = Rc::new(Cell::new(0));
        let text = drain_stderr(Some(Probe {
            left: 900,
            hits: Rc::clone(&hits),
        }));
        assert_eq!(text.len(), 400);
        assert!(text.chars().all(|ch| ch == 'z'));
        assert!(hits.get() > 5);
    }

    struct Probe {
        left: usize,
        hits: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl std::io::Read for Probe {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.hits.set(self.hits.get() + 1);
            if self.left == 0 {
                return Ok(0);
            }
            let n = buf.len().min(self.left).min(100);
            buf[..n].fill(b'z');
            self.left -= n;
            Ok(n)
        }
    }
}
