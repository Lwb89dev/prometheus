//! Local speech routes. Audio is not sent anywhere by this module; the caller
//! picks the endpoint and is expected to keep a microphone on loopback.

use std::time::Duration;

use lda_common::{join_endpoint, wav_is_usable, Error, Result, MAX_AUDIO_BYTES};
use lda_security::redact_secrets;
use serde_json::{json, Value};

use crate::http::provider_err;

const SPEECH_CHARS: usize = 4_096;
const SPEECH_BYTES: usize = 16 * 1024 * 1024;

pub fn transcription_text(body: &Value) -> Result<String> {
    let text = body
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if text.is_empty() {
        return Err(Error::Provider(
            "the speech-to-text model returned no words".into(),
        ));
    }
    if text.chars().count() > 32_000 {
        return Err(Error::Provider("the transcript is too long".into()));
    }
    Ok(text.to_string())
}

pub async fn transcribe_audio(
    http: &reqwest::Client,
    endpoint: &str,
    model: &str,
    wav: &[u8],
) -> Result<String> {
    let model = safe_model(model)?;
    if !wav_is_usable(wav) {
        return Err(Error::InvalidInput(
            "the recording is empty or not a wav. The microphone is closed.".into(),
        ));
    }
    let url = audio_url(endpoint, "transcriptions")?;
    let part = multipart_wav(model, wav)?;
    let response = http
        .post(url)
        .timeout(Duration::from_secs(120))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={}", part.boundary),
        )
        .body(part.bytes)
        .send()
        .await
        .map_err(provider_err)?;
    let status = response.status();
    let text = response.text().await.map_err(provider_err)?;
    if !status.is_success() {
        return Err(provider_err(format!(
            "HTTP {status}: {}",
            audio_error(&text)
        )));
    }
    let value: Value = serde_json::from_str(&text).map_err(provider_err)?;
    transcription_text(&value)
}

pub async fn synthesize_speech(
    http: &reqwest::Client,
    endpoint: &str,
    model: &str,
    text: &str,
) -> Result<Vec<u8>> {
    let model = safe_model(model)?;
    let input = speech_input(text)?;
    let url = audio_url(endpoint, "speech")?;
    let response = http
        .post(url)
        .timeout(Duration::from_secs(120))
        .json(&json!({
            "model": model,
            "input": input,
            "voice": "default",
            "response_format": "wav",
        }))
        .send()
        .await
        .map_err(provider_err)?;
    let status = response.status();
    let bytes = response.bytes().await.map_err(provider_err)?;
    if status.as_u16() == 404 {
        return Err(Error::Provider(
            "this server has no /v1/audio/speech endpoint. Ollama speech needs 0.40.9 or later, and the text was not sent to chat.".into(),
        ));
    }
    if !status.is_success() {
        return Err(provider_err(format!(
            "HTTP {status}: {}",
            audio_error(&String::from_utf8_lossy(&bytes))
        )));
    }
    if bytes.len() > SPEECH_BYTES || bytes.starts_with(b"{") || bytes.len() < 44 {
        return Err(Error::Provider(
            "the speech model did not return a wav".into(),
        ));
    }
    Ok(bytes.to_vec())
}

fn audio_url(endpoint: &str, leaf: &str) -> Result<String> {
    let url = lda_common::parse_endpoint(endpoint)?;
    let path = url.path().trim_end_matches('/');
    let suffix = if path.ends_with("/v1") {
        format!("audio/{leaf}")
    } else {
        format!("v1/audio/{leaf}")
    };
    join_endpoint(endpoint, &suffix)
}

fn speech_input(text: &str) -> Result<&str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInput("nothing to speak".into()));
    }
    if trimmed.contains('\0') {
        return Err(Error::InvalidInput("text contains a NUL byte".into()));
    }
    if trimmed.chars().count() > SPEECH_CHARS {
        return Err(Error::InvalidInput(
            "speech input is limited to 4096 characters".into(),
        ));
    }
    Ok(trimmed)
}

fn safe_model(name: &str) -> Result<&str> {
    let trimmed = name.trim();
    let bad = trimmed.is_empty()
        || trimmed.len() > 256
        || trimmed
            .chars()
            .any(|ch| ch.is_control() || ch == '"' || ch.is_whitespace());
    if bad {
        return Err(Error::InvalidInput("model name is not valid".into()));
    }
    Ok(trimmed)
}

struct Multipart {
    boundary: String,
    bytes: Vec<u8>,
}

fn multipart_wav(model: &str, wav: &[u8]) -> Result<Multipart> {
    if wav.len() > MAX_AUDIO_BYTES {
        return Err(Error::InvalidInput("recording is too long".into()));
    }
    let boundary = boundary_for(wav);
    let mut bytes = Vec::with_capacity(wav.len() + 256);
    push_text(&mut bytes, &boundary, "model", model);
    bytes.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    bytes.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\n",
    );
    bytes.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
    bytes.extend_from_slice(wav);
    bytes.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Ok(Multipart { boundary, bytes })
}

fn push_text(bytes: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    bytes.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        )
        .as_bytes(),
    );
}

fn boundary_for(wav: &[u8]) -> String {
    let mut n = 0u32;
    loop {
        let boundary = format!("ldaBoundary{n}{}", lda_common::new_id());
        if !wav
            .windows(boundary.len())
            .any(|window| window == boundary.as_bytes())
        {
            return boundary;
        }
        n += 1;
        if n > 4 {
            return boundary;
        }
    }
}

fn audio_error(text: &str) -> String {
    let redacted = redact_secrets(text);
    if let Ok(value) = serde_json::from_str::<Value>(&redacted) {
        if let Some(message) = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| value.get("error").and_then(Value::as_str))
        {
            return clip(message);
        }
    }
    clip(&redacted)
}

fn clip(text: &str) -> String {
    let mut out: String = text.chars().take(300).collect();
    if text.chars().count() > 300 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_json_reads_text() {
        let value = json!({"text": " hello "});
        assert_eq!(transcription_text(&value).unwrap(), "hello");
        assert!(transcription_text(&json!({"text": ""})).is_err());
    }

    #[tokio::test]
    async fn local_transcription_and_speech_round_trip() {
        let wav = sample_wav();
        let transcribe = serve(200, b"{\"text\":\"hello there\"}", "whisper:latest");
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let text = transcribe_audio(&http, &transcribe, "whisper:latest", &wav)
            .await
            .unwrap();
        assert_eq!(text, "hello there");
        let speech = serve(
            200,
            b"RIFF0123WAVEabcdefghijklmnopqrstuvwxyz0123456789",
            "phonon",
        );
        let audio = synthesize_speech(&http, &speech, "phonon", "Hello")
            .await
            .unwrap();
        assert!(audio.starts_with(b"RIFF"));
        let missing = serve(404, b"404 page not found", "");
        let err = synthesize_speech(&http, &missing, "phonon", "Hello")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("0.40.9"));
    }

    fn sample_wav() -> Vec<u8> {
        let mut wav = b"RIFF....WAVEfmt ".to_vec();
        wav.resize(64, 0);
        wav
    }

    fn serve(status: u16, body: &'static [u8], needle: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || answer(listener, status, body, needle));
        format!("http://{addr}")
    }

    fn answer(listener: std::net::TcpListener, status: u16, body: &[u8], needle: &str) {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let request = read_request(&mut socket);
        let text = String::from_utf8_lossy(&request);
        assert!(
            text.contains(needle)
                || request
                    .windows(needle.len())
                    .any(|w| w == needle.as_bytes()),
            "request did not mention {needle}"
        );
        let header = format!(
            "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        use std::io::Write;
        let _ = socket.write_all(header.as_bytes());
        let _ = socket.write_all(body);
    }

    fn read_request(socket: &mut std::net::TcpStream) -> Vec<u8> {
        use std::io::Read;
        let mut buf = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            if request_complete(&buf) {
                return buf;
            }
            match socket.read(&mut tmp) {
                Ok(0) => return buf,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(err)
                    if err.kind() == std::io::ErrorKind::WouldBlock
                        || err.kind() == std::io::ErrorKind::TimedOut =>
                {
                    return buf;
                }
                Err(_) => return buf,
            }
        }
    }

    fn request_complete(buf: &[u8]) -> bool {
        let Some(end) = find_subsequence(buf, b"\r\n\r\n") else {
            return false;
        };
        let header = String::from_utf8_lossy(&buf[..end]);
        let Some(len) = content_length(&header) else {
            return false;
        };
        buf.len() >= end + 4 + len
    }

    fn content_length(header: &str) -> Option<usize> {
        for line in header.lines() {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.eq_ignore_ascii_case("content-length") {
                return value.trim().parse().ok();
            }
        }
        None
    }

    fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }
}
