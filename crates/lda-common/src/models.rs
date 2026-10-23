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
}

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
}
