//! Voice stays optional and off. No backend is linked, so nothing opens a microphone.

pub trait VoiceBackend: Send + Sync {
    fn available(&self) -> bool;
    fn push_to_talk(&self) -> bool;
    fn wake_word(&self) -> bool;
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_is_off() {
        let voice = NoVoice;
        assert!(!voice.available());
        assert!(!voice.push_to_talk());
        assert!(!voice.wake_word());
    }
}
