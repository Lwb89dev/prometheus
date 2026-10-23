//! Local model choice. Nothing here contacts a server or downloads a file.
//!
//! A `:latest` tag is a moving name. The generated configuration does not
//! pick one, and a download is offered only for a concrete catalog tag.

use serde::{Deserialize, Serialize};

/// One model the local server already has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledModel {
    pub name: String,
    pub size_bytes: u64,
    pub family: String,
    pub parameter_size: String,
    /// `Some` when the server reported a capability list. `None` means unknown.
    pub tools: Option<bool>,
    /// Names from that capability list. Empty means the server did not give one.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// What a model may be asked to do. A speech model is not a chat model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelKind {
    pub chat: bool,
    pub transcribe: bool,
    pub speak: bool,
}

/// What the window does with text that came from the microphone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptNext {
    Chat,
    Insert,
    Speak,
}

/// Transcription result. `follow_up` is `chat`, `insert`, or `speak`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transcript {
    pub text: String,
    pub model: String,
    pub follow_up: String,
}

/// Recorded audio stays under this size. One minute of 16 kHz mono is about 2 MiB.
pub const MAX_AUDIO_BYTES: usize = 8 * 1024 * 1024;

/// A concrete tag the user may approve for download. Not a default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSuggestion {
    pub name: String,
    pub min_ram_mib: u64,
    pub tools: bool,
    pub reason: String,
}

/// Installed models plus tags that fit the free RAM. `notice` is empty when
/// the local server answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryReport {
    pub installed: Vec<InstalledModel>,
    pub suggestions: Vec<ModelSuggestion>,
    pub ram_mib: u64,
    pub notice: String,
}

struct CatalogEntry {
    name: &'static str,
    min_ram_mib: u64,
    note: &'static str,
}

/// Concrete Ollama library tags. Sizes are a conservative floor for a 4-bit
/// weight file plus runtime overhead, not a measured benchmark.
const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        name: "qwen2.5:1.5b",
        min_ram_mib: 2_048,
        note: "Small Qwen 2.5 tag. This family is documented for Ollama tool calls.",
    },
    CatalogEntry {
        name: "qwen2.5:3b",
        min_ram_mib: 4_096,
        note: "Qwen 2.5 3B tag. This family is documented for Ollama tool calls.",
    },
    CatalogEntry {
        name: "llama3.2:3b",
        min_ram_mib: 4_096,
        note: "Llama 3.2 3B tag. This family is documented for Ollama tool calls.",
    },
    CatalogEntry {
        name: "qwen2.5:7b",
        min_ram_mib: 8_192,
        note: "Qwen 2.5 7B tag. This family is documented for Ollama tool calls.",
    },
    CatalogEntry {
        name: "llama3.1:8b",
        min_ram_mib: 8_192,
        note: "Llama 3.1 8B tag. This family is documented for Ollama tool calls.",
    },
    CatalogEntry {
        name: "qwen2.5:14b",
        min_ram_mib: 16_384,
        note: "Qwen 2.5 14B tag. This family is documented for Ollama tool calls.",
    },
    CatalogEntry {
        name: "qwen2.5:32b",
        min_ram_mib: 24_576,
        note: "Qwen 2.5 32B tag. This family is documented for Ollama tool calls.",
    },
];

pub fn model_is_unset(model: &str) -> bool {
    model.is_empty()
}

pub fn is_catalog_model(name: &str) -> bool {
    CATALOG.iter().any(|entry| entry.name == name)
}

/// Tool-capable models first, then unknown, then models that reported no
/// tools. A concrete tag sorts ahead of `:latest` inside the same group.
pub fn rank_installed(models: &mut [InstalledModel]) {
    models.sort_by(|left, right| {
        tool_rank(right.tools)
            .cmp(&tool_rank(left.tools))
            .then_with(|| tag_rank(&right.name).cmp(&tag_rank(&left.name)))
            .then_with(|| left.name.cmp(&right.name))
    });
}

/// Suggestions that fit `ram_mib`. Already installed names are omitted.
/// When nothing fits, the smallest missing tag is still listed so the user
/// can see the floor. An empty RAM reading lists the two smallest tags.
pub fn suggest_for_ram(ram_mib: u64, installed: &[String]) -> Vec<ModelSuggestion> {
    let missing: Vec<&CatalogEntry> = CATALOG
        .iter()
        .filter(|entry| !installed.iter().any(|name| name == entry.name))
        .collect();
    if ram_mib == 0 {
        return missing
            .into_iter()
            .take(2)
            .map(|entry| {
                suggestion(
                    entry,
                    "Free RAM was not read. This is one of the smaller tags.",
                )
            })
            .collect();
    }
    let mut fits: Vec<&CatalogEntry> = missing
        .iter()
        .copied()
        .filter(|entry| entry.min_ram_mib <= ram_mib)
        .collect();
    if fits.is_empty() {
        return missing
            .into_iter()
            .take(1)
            .map(|entry| {
                suggestion(
                    entry,
                    "Free RAM is below this tag's floor. Downloading it still needs your approval.",
                )
            })
            .collect();
    }
    fits.sort_by_key(|entry| std::cmp::Reverse(entry.min_ram_mib));
    fits.truncate(4);
    fits.into_iter()
        .map(|entry| suggestion(entry, entry.note))
        .collect()
}

fn suggestion(entry: &CatalogEntry, reason: &str) -> ModelSuggestion {
    ModelSuggestion {
        name: entry.name.to_string(),
        min_ram_mib: entry.min_ram_mib,
        tools: true,
        reason: format!(
            "{reason} About {} MiB free is the floor used here.",
            entry.min_ram_mib
        ),
    }
}

fn tool_rank(tools: Option<bool>) -> u8 {
    match tools {
        Some(true) => 2,
        None => 1,
        Some(false) => 0,
    }
}

fn tag_rank(name: &str) -> u8 {
    if name.ends_with(":latest") {
        return 0;
    }
    1
}

pub fn kind_of(model: &InstalledModel) -> ModelKind {
    model_kind(&model.capabilities, &model.name)
}

/// Classify a model from the server's capability list.
///
/// An empty list falls back to the name: `whisper*` transcribes, `phonon*`
/// speaks, and every other name stays a conversation model so an older server
/// that omits capabilities still chats. `speech` never chats. Ollama answers
/// chat, completion, and embeddings to a speech model with HTTP 400.
pub fn model_kind(capabilities: &[String], name: &str) -> ModelKind {
    if capabilities.is_empty() {
        return kind_from_name(name);
    }
    let speak = has(capabilities, "speech");
    let completion = has(capabilities, "completion");
    let transcribe = has(capabilities, "transcription")
        || has(capabilities, "transcribe")
        || (has(capabilities, "audio") && !completion);
    ModelKind {
        chat: completion && !speak,
        transcribe,
        speak,
    }
}

pub fn transcript_next(kind: ModelKind) -> TranscriptNext {
    if kind.chat {
        return TranscriptNext::Chat;
    }
    if kind.speak {
        return TranscriptNext::Speak;
    }
    TranscriptNext::Insert
}

impl TranscriptNext {
    pub fn as_str(self) -> &'static str {
        match self {
            TranscriptNext::Chat => "chat",
            TranscriptNext::Insert => "insert",
            TranscriptNext::Speak => "speak",
        }
    }
}

impl ModelKind {
    pub fn label(self) -> &'static str {
        if self.chat && self.transcribe {
            return "conversation, hears speech";
        }
        if self.chat {
            return "conversation";
        }
        if self.transcribe && self.speak {
            return "speech-to-text and speech";
        }
        if self.transcribe {
            return "speech-to-text";
        }
        if self.speak {
            return "speech";
        }
        "no conversation, speech, or transcription"
    }
}

pub fn chat_refused(name: &str, kind: ModelKind) -> String {
    if kind.speak && !kind.chat {
        return format!("{name} generates speech and does not take a chat. The text was not sent.");
    }
    if kind.transcribe && !kind.chat {
        return format!(
            "{name} transcribes speech and does not take a chat. The text was not sent."
        );
    }
    format!("{name} has no conversation capability. The text was not sent.")
}

/// Prefer the selected model when it transcribes. Otherwise use the only
/// installed transcriber. Several transcribers need an explicit choice.
pub fn pick_transcriber(selected: &str, installed: &[InstalledModel]) -> Result<String, String> {
    if let Some(model) = installed.iter().find(|model| model.name == selected) {
        if kind_of(model).transcribe {
            return Ok(model.name.clone());
        }
    } else if model_kind(&[], selected).transcribe {
        return Ok(selected.to_string());
    }
    let others: Vec<&str> = installed
        .iter()
        .filter(|model| kind_of(model).transcribe)
        .map(|model| model.name.as_str())
        .collect();
    if others.len() == 1 {
        return Ok(others[0].to_string());
    }
    if others.is_empty() {
        return Err(format!(
            "{selected} does not transcribe speech, and no speech-to-text model is installed."
        ));
    }
    Err(format!(
        "Several speech-to-text models are installed ({}). Select one of them to transcribe.",
        others.join(", ")
    ))
}

pub fn wav_is_usable(bytes: &[u8]) -> bool {
    bytes.len() >= 64
        && bytes.len() <= MAX_AUDIO_BYTES
        && bytes.starts_with(b"RIFF")
        && bytes.get(8..12) == Some(b"WAVE".as_slice())
}

fn has(capabilities: &[String], name: &str) -> bool {
    capabilities.iter().any(|item| item == name)
}

fn kind_from_name(name: &str) -> ModelKind {
    let base = name
        .split(':')
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_lowercase();
    if stt_name(&base) {
        return ModelKind {
            chat: false,
            transcribe: true,
            speak: false,
        };
    }
    if speech_name(&base) {
        return ModelKind {
            chat: false,
            transcribe: false,
            speak: true,
        };
    }
    ModelKind {
        chat: true,
        transcribe: false,
        speak: false,
    }
}

fn stt_name(base: &str) -> bool {
    base.starts_with("whisper") || base.contains("transcribe") || base.contains("distil-whisper")
}

fn speech_name(base: &str) -> bool {
    base == "phonon" || base.starts_with("phonon")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(name: &str, tools: Option<bool>) -> InstalledModel {
        InstalledModel {
            name: name.to_string(),
            size_bytes: 1,
            family: String::new(),
            parameter_size: String::new(),
            tools,
            capabilities: Vec::new(),
        }
    }

    #[test]
    fn default_choice_is_unset_and_catalog_has_no_latest_tag() {
        assert!(model_is_unset(""));
        assert!(!model_is_unset("qwen2.5:7b"));
        assert!(CATALOG.iter().all(|entry| !entry.name.ends_with(":latest")));
        assert!(is_catalog_model("qwen2.5:7b"));
        assert!(!is_catalog_model("mistral:latest"));
        assert!(!is_catalog_model("qwen2.5"));
    }

    #[test]
    fn tool_capable_concrete_tags_sort_first() {
        let mut models = vec![
            installed("mistral:latest", Some(false)),
            installed("llama3.1:8b", None),
            installed("qwen2.5:7b", Some(true)),
            installed("old:latest", Some(true)),
        ];
        rank_installed(&mut models);
        let names: Vec<&str> = models.iter().map(|model| model.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["qwen2.5:7b", "old:latest", "llama3.1:8b", "mistral:latest"]
        );
    }

    #[test]
    fn suggestions_follow_free_ram_and_skip_installed_names() {
        let installed = vec!["qwen2.5:7b".to_string()];
        let found = suggest_for_ram(8_192, &installed);
        let names: Vec<&str> = found.iter().map(|item| item.name.as_str()).collect();
        assert!(names.contains(&"llama3.1:8b"));
        assert!(!names.contains(&"qwen2.5:7b"));
        assert!(!names.contains(&"qwen2.5:14b"));
        assert!(found.iter().all(|item| item.tools));
    }

    #[test]
    fn low_ram_still_shows_the_smallest_tag() {
        let found = suggest_for_ram(512, &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "qwen2.5:1.5b");
    }

    #[test]
    fn capabilities_separate_chat_stt_and_speech() {
        let chat = model_kind(&["completion".into(), "tools".into()], "mistral:latest");
        assert!(chat.chat);
        assert!(!chat.transcribe);
        assert!(!chat.speak);
        let heard = model_kind(
            &["completion".into(), "transcription".into()],
            "voice:latest",
        );
        assert!(heard.chat && heard.transcribe);
        assert_eq!(transcript_next(heard), TranscriptNext::Chat);
        let stt = model_kind(&["transcription".into()], "whisper:latest");
        assert!(stt.transcribe && !stt.chat);
        assert_eq!(transcript_next(stt), TranscriptNext::Insert);
        let speech = model_kind(&["speech".into()], "phonon");
        assert!(speech.speak && !speech.chat);
        assert_eq!(transcript_next(speech), TranscriptNext::Speak);
        assert!(model_kind(&[], "whisper-small").transcribe);
        assert!(model_kind(&[], "phonon:latest").speak);
        assert!(model_kind(&[], "mistral:latest").chat);
    }

    #[test]
    fn one_transcriber_is_used_beside_a_conversation_model() {
        let installed = vec![
            with_caps("mistral:latest", &["completion", "tools"]),
            with_caps("whisper:latest", &["transcription"]),
            with_caps("other-whisper:latest", &["transcription"]),
        ];
        let err = pick_transcriber("mistral:latest", &installed).unwrap_err();
        assert!(err.contains("Several"));
        let only = vec![installed[0].clone(), installed[1].clone()];
        assert_eq!(
            pick_transcriber("mistral:latest", &only).unwrap(),
            "whisper:latest"
        );
        assert_eq!(
            pick_transcriber("whisper:latest", &installed).unwrap(),
            "whisper:latest"
        );
        assert!(pick_transcriber("mistral:latest", &only[..1]).is_err());
    }

    fn with_caps(name: &str, caps: &[&str]) -> InstalledModel {
        let mut model = installed(name, None);
        model.capabilities = caps.iter().map(|item| (*item).to_string()).collect();
        model
    }

    #[test]
    fn wav_header_and_size_are_checked() {
        let mut bytes = b"RIFF....WAVErest-of-header-padding-makes-sixty-four-bytes!!".to_vec();
        bytes.resize(64, b' ');
        assert!(wav_is_usable(&bytes));
        bytes[8] = b'X';
        assert!(!wav_is_usable(&bytes));
        assert!(!wav_is_usable(b"RIFF"));
    }
}
