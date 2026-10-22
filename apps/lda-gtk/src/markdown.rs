//! Markdown to styled spans. The text view never interprets HTML or links as actions.

use gtk::{glib::translate::IntoGlib, prelude::*};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    Text,
    Strong,
    Emphasis,
    Code,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub kind: SpanKind,
}

pub fn markdown_spans(input: &str) -> Vec<Span> {
    let mut state = MdState::default();
    let parser = Parser::new_ext(input, Options::empty());
    for event in parser {
        on_event(&mut state, event);
    }
    state.spans
}

#[derive(Default)]
struct MdState {
    spans: Vec<Span>,
    kind: Option<SpanKind>,
    href: Option<String>,
}

fn on_event(state: &mut MdState, event: Event<'_>) {
    match event {
        Event::Start(tag) => start_tag(state, tag),
        Event::End(tag) => end_tag(state, tag),
        Event::Text(text) => push(state, &text, state.kind.unwrap_or(SpanKind::Text)),
        Event::Code(text) => push(state, &text, SpanKind::Code),
        Event::SoftBreak | Event::HardBreak => push(state, "\n", SpanKind::Text),
        Event::Rule => push(state, "\n———\n", SpanKind::Text),
        Event::Html(text) | Event::InlineHtml(text) => push(state, &text, SpanKind::Text),
        Event::TaskListMarker(checked) => push_task(state, checked),
        Event::FootnoteReference(text) => push(state, &text, SpanKind::Text),
        Event::InlineMath(text) | Event::DisplayMath(text) => push(state, &text, SpanKind::Code),
    }
}

fn start_tag(state: &mut MdState, tag: Tag<'_>) {
    match tag {
        Tag::Strong => state.kind = Some(SpanKind::Strong),
        Tag::Emphasis => state.kind = Some(SpanKind::Emphasis),
        Tag::Strikethrough => state.kind = Some(SpanKind::Emphasis),
        Tag::CodeBlock(_) => state.kind = Some(SpanKind::Code),
        Tag::Heading { .. } => state.kind = Some(SpanKind::Strong),
        Tag::Link { dest_url, .. } => {
            state.kind = Some(SpanKind::Link);
            state.href = Some(dest_url.to_string());
        }
        Tag::Paragraph | Tag::Item => push(state, "", SpanKind::Text),
        _ => {}
    }
}

fn end_tag(state: &mut MdState, tag: TagEnd) {
    match tag {
        TagEnd::Link => finish_link(state),
        TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::Item => {
            state.kind = None;
            push(state, "\n", SpanKind::Text);
        }
        TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough => state.kind = None,
        _ => {}
    }
}

fn finish_link(state: &mut MdState) {
    if let Some(href) = state.href.take() {
        push(state, &format!(" ({href})"), SpanKind::Text);
    }
    state.kind = None;
}

fn push_task(state: &mut MdState, checked: bool) {
    if checked {
        push(state, "[x] ", SpanKind::Text);
        return;
    }
    push(state, "[ ] ", SpanKind::Text);
}

pub fn render_transcript(buffer: &gtk::TextBuffer, messages: &[lda_common::StoredMessage]) {
    ensure_tags(buffer);
    buffer.set_text("");
    for message in messages {
        if message.role == lda_common::Role::System {
            continue;
        }
        insert(buffer, &format!("{}\n", heading(message)), Some("strong"));
        render_body(buffer, message);
        insert(buffer, "\n", None);
    }
}

fn heading(message: &lda_common::StoredMessage) -> String {
    match message.role {
        lda_common::Role::User => "You".to_string(),
        lda_common::Role::Tool => {
            format!("Tool {}", message.tool_name.as_deref().unwrap_or("result"))
        }
        lda_common::Role::Assistant if !message.tool_calls.is_empty() => {
            let names = message
                .tool_calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("Assistant requested {names}")
        }
        lda_common::Role::Assistant => "Assistant".to_string(),
        lda_common::Role::System => "System".to_string(),
    }
}

fn render_body(buffer: &gtk::TextBuffer, message: &lda_common::StoredMessage) {
    if message.content.is_empty() {
        return;
    }
    if message.role == lda_common::Role::Assistant {
        for span in markdown_spans(&message.content) {
            insert(buffer, &span.text, tag_name(span.kind));
        }
        return;
    }
    let tag = if message.role == lda_common::Role::Tool {
        Some("code")
    } else {
        None
    };
    insert(buffer, &message.content, tag);
    insert(buffer, "\n", None);
}

fn tag_name(kind: SpanKind) -> Option<&'static str> {
    match kind {
        SpanKind::Strong => Some("strong"),
        SpanKind::Emphasis => Some("em"),
        SpanKind::Code => Some("code"),
        SpanKind::Link => Some("link"),
        SpanKind::Text => None,
    }
}

fn ensure_tags(buffer: &gtk::TextBuffer) {
    let table = buffer.tag_table();
    if table.lookup("strong").is_some() {
        return;
    }
    add_tag(
        buffer,
        "strong",
        gtk::pango::Weight::Bold,
        gtk::pango::Style::Normal,
        false,
    );
    add_tag(
        buffer,
        "em",
        gtk::pango::Weight::Normal,
        gtk::pango::Style::Italic,
        false,
    );
    add_tag(
        buffer,
        "code",
        gtk::pango::Weight::Normal,
        gtk::pango::Style::Normal,
        true,
    );
    add_tag(
        buffer,
        "link",
        gtk::pango::Weight::Normal,
        gtk::pango::Style::Normal,
        false,
    );
}

fn add_tag(
    buffer: &gtk::TextBuffer,
    name: &str,
    weight: gtk::pango::Weight,
    style: gtk::pango::Style,
    mono: bool,
) {
    let tag = gtk::TextTag::builder()
        .name(name)
        .weight(weight.into_glib())
        .style(style)
        .build();
    if mono {
        tag.set_family(Some("monospace"));
    }
    buffer.tag_table().add(&tag);
}

fn insert(buffer: &gtk::TextBuffer, text: &str, tag: Option<&str>) {
    let mut end = buffer.end_iter();
    if let Some(name) = tag {
        buffer.insert_with_tags_by_name(&mut end, text, &[name]);
        return;
    }
    buffer.insert(&mut end, text);
}

fn push(state: &mut MdState, text: &str, kind: SpanKind) {
    if text.is_empty() {
        return;
    }
    state.spans.push(Span {
        text: text.to_string(),
        kind,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_and_links_are_spans_not_actions() {
        let spans = markdown_spans("See [docs](https://example.com).\n\n```\nls\n```\n");
        assert!(spans
            .iter()
            .any(|span| span.kind == SpanKind::Link && span.text == "docs"));
        assert!(spans
            .iter()
            .any(|span| span.text.contains("https://example.com")));
        assert!(spans
            .iter()
            .any(|span| span.kind == SpanKind::Code && span.text.contains("ls")));
        let raw = markdown_spans("<script>alert(1)</script>");
        let joined: String = raw.iter().map(|span| span.text.as_str()).collect();
        assert!(joined.contains("script"));
        assert!(raw.iter().all(|span| span.kind == SpanKind::Text));
    }
}
