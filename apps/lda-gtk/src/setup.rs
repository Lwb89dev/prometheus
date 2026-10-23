//! Local model choice. Download starts only after the user confirms a catalog tag.

use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;
use lda_common::{is_catalog_model, DiscoveryReport, ProviderConfig, ProviderKind};

use crate::bus::Command;
use crate::window::{set_status, Ctx};

pub fn open(ctx: &Rc<Ctx>) {
    if ctx.model.borrow().setup_dialog.is_some() {
        ctx.command(Command::Discover {
            id: provider_id(ctx),
        });
        return;
    }
    let installed = gtk::ListBox::new();
    installed.set_selection_mode(gtk::SelectionMode::Browse);
    installed.update_property(&[gtk::accessible::Property::Label("Installed models")]);
    let suggestions = gtk::ListBox::new();
    suggestions.set_selection_mode(gtk::SelectionMode::Browse);
    suggestions.update_property(&[gtk::accessible::Property::Label("Suggested models")]);
    let notice = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .label("Looking for local models. Nothing is downloaded until you approve a tag.")
        .build();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.append(&notice);
    root.append(&section("Installed on this Ollama server"));
    root.append(&scrolled(&installed));
    root.append(&section("Suggested tags that fit free RAM"));
    root.append(&scrolled(&suggestions));
    let use_button = gtk::Button::with_label("Use installed model");
    let download = gtk::Button::with_label("Download suggested tag");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.append(&use_button);
    buttons.append(&download);
    root.append(&buttons);
    let dialog = adw::Dialog::builder()
        .title("Local model")
        .content_width(520)
        .content_height(560)
        .child(&root)
        .build();
    {
        let mut model = ctx.model.borrow_mut();
        model.installed_list = Some(installed);
        model.suggestion_list = Some(suggestions);
        model.setup_notice = Some(notice);
        model.setup_dialog = Some(dialog.clone());
    }
    let ctx_close = Rc::clone(ctx);
    dialog.connect_closed(move |_| clear_setup(&ctx_close));
    let ctx_use = Rc::clone(ctx);
    use_button.connect_clicked(move |_| use_selected(&ctx_use));
    let ctx_download = Rc::clone(ctx);
    download.connect_clicked(move |_| confirm_download(&ctx_download));
    ctx.command(Command::Discover {
        id: provider_id(ctx),
    });
    dialog.present(Some(&ctx.widgets.window));
}

pub fn show(ctx: &Ctx, report: &DiscoveryReport) {
    let Some(installed) = ctx.model.borrow().installed_list.clone() else {
        return;
    };
    let Some(suggestions) = ctx.model.borrow().suggestion_list.clone() else {
        return;
    };
    fill_installed(&installed, report);
    fill_suggestions(&suggestions, report);
    let text = notice_text(report);
    if let Some(label) = ctx.model.borrow().setup_notice.clone() {
        label.set_text(&text);
    }
}

pub fn pull_progress(ctx: &Rc<Ctx>, phase: &str, name: &str, detail: &str) {
    let text = progress_text(name, detail);
    if let Some(label) = ctx.model.borrow().setup_notice.clone() {
        label.set_text(&text);
    }
    set_status(&ctx.widgets, &text);
    if phase == "running" {
        schedule_poll(ctx);
        return;
    }
    if phase == "succeeded" && ctx.model.borrow().setup_dialog.is_some() {
        ctx.command(Command::Discover {
            id: provider_id(ctx),
        });
    }
}

fn progress_text(name: &str, detail: &str) -> String {
    if name.is_empty() || detail.contains(name) {
        return detail.to_string();
    }
    if detail.is_empty() {
        return name.to_string();
    }
    format!("{name}: {detail}")
}

fn schedule_poll(ctx: &Rc<Ctx>) {
    let ctx = Rc::clone(ctx);
    glib::timeout_add_local(Duration::from_secs(1), move || {
        ctx.command(Command::PullStatus);
        glib::ControlFlow::Break
    });
}

fn fill_installed(list: &gtk::ListBox, report: &DiscoveryReport) {
    clear(list);
    if report.installed.is_empty() {
        list.append(&plain("No installed model was reported."));
        return;
    }
    for model in &report.installed {
        let tools = match model.tools {
            Some(true) => "native tools",
            Some(false) => "no native tools reported",
            None => "tools unknown",
        };
        let mut label = format!("{}\n{tools}. {}", model.name, size_label(model.size_bytes));
        if model.name.ends_with(":latest") {
            label.push_str(" Tag :latest can change.");
        }
        list.append(&named_row(&model.name, &label));
    }
}

fn fill_suggestions(list: &gtk::ListBox, report: &DiscoveryReport) {
    clear(list);
    if report.suggestions.is_empty() {
        list.append(&plain("No suggestion fits this machine."));
        return;
    }
    for item in &report.suggestions {
        let label = format!(
            "{}\nFloor {} MiB. {}",
            item.name, item.min_ram_mib, item.reason
        );
        list.append(&named_row(&item.name, &label));
    }
}

fn notice_text(report: &DiscoveryReport) -> String {
    let mut text = format!("Free RAM used for suggestions: {} MiB.", report.ram_mib);
    if !report.notice.is_empty() {
        text.push(' ');
        text.push_str(&report.notice);
    }
    text
}

fn use_selected(ctx: &Rc<Ctx>) {
    let Some(list) = ctx.model.borrow().installed_list.clone() else {
        return;
    };
    let Some(name) = selected_name(&list) else {
        set_status(&ctx.widgets, "Select an installed model first.");
        return;
    };
    save_model(ctx, &name);
}

fn confirm_download(ctx: &Rc<Ctx>) {
    let Some(list) = ctx.model.borrow().suggestion_list.clone() else {
        return;
    };
    let Some(name) = selected_name(&list) else {
        set_status(&ctx.widgets, "Select a suggested tag first.");
        return;
    };
    if !is_catalog_model(&name) {
        set_status(
            &ctx.widgets,
            "That tag is not one of the suggested downloads.",
        );
        return;
    }
    let message = format!("Download {name}?");
    let detail = "This contacts the loopback Ollama server and can use a lot of disk and network. Nothing is downloaded until you continue.";
    confirm(ctx, &message, detail, move |ctx| {
        ctx.command(Command::StartPull {
            id: provider_id(ctx),
            name: name.clone(),
        });
    });
}

fn save_model(ctx: &Ctx, name: &str) {
    let Some(mut provider) = current_provider(ctx) else {
        set_status(&ctx.widgets, "The local provider is not loaded yet.");
        return;
    };
    if provider.kind != ProviderKind::Ollama {
        set_status(
            &ctx.widgets,
            "Model discovery saves the local Ollama provider. Cloud providers stay unchanged.",
        );
        return;
    }
    provider.model = name.to_string();
    let json = match serde_json::to_string(&provider) {
        Ok(json) => json,
        Err(err) => {
            set_status(&ctx.widgets, &err.to_string());
            return;
        }
    };
    ctx.model.borrow_mut().provider_id = provider.id;
    ctx.command(Command::UpsertProvider { json });
    set_status(&ctx.widgets, &format!("Using {name}."));
    if let Some(dialog) = ctx.model.borrow().setup_dialog.clone() {
        dialog.close();
    }
}

fn current_provider(ctx: &Ctx) -> Option<ProviderConfig> {
    let model = ctx.model.borrow();
    model
        .providers
        .iter()
        .find(|provider| provider.id == model.provider_id)
        .cloned()
        .or_else(|| model.providers.first().cloned())
}

fn provider_id(ctx: &Ctx) -> String {
    let model = ctx.model.borrow();
    if model.provider_id.is_empty() {
        return "ollama".to_string();
    }
    model.provider_id.clone()
}

fn clear_setup(ctx: &Ctx) {
    let mut model = ctx.model.borrow_mut();
    model.installed_list = None;
    model.suggestion_list = None;
    model.setup_notice = None;
    model.setup_dialog = None;
}

fn selected_name(list: &gtk::ListBox) -> Option<String> {
    let row = list.selected_row()?;
    let child = row.child()?;
    let marked = child.widget_name();
    let name = marked.strip_prefix("lda-model-")?;
    if name.is_empty() || name.len() > 256 || name.chars().any(|ch| ch.is_control()) {
        return None;
    }
    Some(name.to_string())
}

fn named_row(name: &str, label: &str) -> gtk::Label {
    let row = gtk::Label::builder()
        .label(label)
        .wrap(true)
        .xalign(0.0)
        .build();
    row.set_margin_start(8);
    row.set_margin_top(6);
    row.set_margin_bottom(6);
    row.set_widget_name(&format!("lda-model-{name}"));
    row
}

fn plain(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).build()
}

fn section(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(["heading"])
        .build()
}

fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .min_content_height(120)
        .vexpand(true)
        .build()
}

fn size_label(bytes: u64) -> String {
    if bytes == 0 {
        return "size unknown".to_string();
    }
    format!("{} MiB", bytes / (1024 * 1024))
}

fn clear(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn confirm(ctx: &Rc<Ctx>, message: &str, detail: &str, action: impl FnOnce(&Ctx) + 'static) {
    let dialog = adw::AlertDialog::builder()
        .heading(message)
        .body(detail)
        .close_response("cancel")
        .default_response("cancel")
        .build();
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("continue", "Continue");
    dialog.set_response_appearance("continue", adw::ResponseAppearance::Suggested);
    let parent = ctx.widgets.window.clone();
    let ctx = Rc::clone(ctx);
    dialog.choose(
        Some(&parent),
        None::<&gtk::gio::Cancellable>,
        move |response| {
            if response.as_str() == "continue" {
                action(&ctx);
            }
        },
    );
}
