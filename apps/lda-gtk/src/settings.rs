//! Provider editor. Secrets go to the daemon's secret-service call and are not written
//! into the configuration file. The entry is cleared after a successful hand-off.

use std::rc::Rc;

use gtk::{glib, prelude::*};
use lda_common::{ProviderConfig, ProviderKind, ToolCallingMode};

use crate::bus::Command;
use crate::window::Ctx;

struct Form {
    root: gtk::Box,
    id: gtk::Entry,
    kind: gtk::DropDown,
    endpoint: gtk::Entry,
    model: gtk::Entry,
    temperature: gtk::SpinButton,
    timeout: gtk::SpinButton,
    share: gtk::CheckButton,
    auth_env: gtk::Entry,
    secret: gtk::PasswordEntry,
    secret_status: gtk::Label,
}

pub fn open(ctx: &Rc<Ctx>) {
    let provider = current_provider(ctx);
    let dialog = gtk::Window::builder()
        .transient_for(&ctx.widgets.window)
        .modal(true)
        .title("Provider")
        .default_width(520)
        .build();
    let form = build_form(&provider);
    dialog.set_child(Some(&form.root));
    ctx.model.borrow_mut().secret_label = Some(form.secret_status.clone());
    hook_close(ctx, &dialog);
    hook_buttons(ctx, &form);
    ctx.command(Command::SecretStatus { id: provider.id });
    dialog.present();
}

fn current_provider(ctx: &Ctx) -> ProviderConfig {
    let model = ctx.model.borrow();
    if let Some(provider) = model
        .providers
        .iter()
        .find(|provider| provider.id == model.provider_id)
    {
        return provider.clone();
    }
    model
        .providers
        .first()
        .cloned()
        .unwrap_or_else(default_provider)
}

fn default_provider() -> ProviderConfig {
    lda_common::FileConfig::default().providers.remove(0)
}

fn build_form(provider: &ProviderConfig) -> Form {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    let grid = gtk::Grid::new();
    grid.set_column_spacing(8);
    grid.set_row_spacing(8);
    let id = entry(&provider.id);
    let kind = gtk::DropDown::from_strings(&["ollama", "openai_compatible"]);
    kind.set_selected(kind_index(provider.kind));
    let endpoint = entry(&provider.endpoint);
    let model = entry(&provider.model);
    let temperature = gtk::SpinButton::with_range(0.0, 2.0, 0.1);
    temperature.set_digits(2);
    temperature.set_value(f64::from(provider.temperature.unwrap_or(0.2)));
    let timeout = gtk::SpinButton::with_range(1.0, 3600.0, 1.0);
    timeout.set_value(provider.timeout_secs as f64);
    let share = gtk::CheckButton::with_label("Send system tool results to this provider");
    share.set_active(provider.share_system_context);
    let auth_env = entry(provider.auth_env.as_deref().unwrap_or(""));
    let secret = gtk::PasswordEntry::new();
    secret.set_show_peek_icon(true);
    let secret_status = gtk::Label::builder().label("Key: …").xalign(0.0).build();
    attach(&grid, 0, "Id", &id);
    attach(&grid, 1, "Kind", &kind);
    attach(&grid, 2, "Endpoint", &endpoint);
    attach(&grid, 3, "Model", &model);
    attach(&grid, 4, "Temperature", &temperature);
    attach(&grid, 5, "Timeout (seconds)", &timeout);
    attach(&grid, 6, "Auth env var", &auth_env);
    grid.attach(&share, 0, 7, 2, 1);
    let note = gtk::Label::builder()
        .label("Loopback providers can use system tools without the checkbox. For any other endpoint, leave it off unless you want host details to leave this computer. API keys are not saved in the configuration file.")
        .wrap(true)
        .xalign(0.0)
        .build();
    let save = gtk::Button::with_label("Save provider");
    let save_key = gtk::Button::with_label("Save key");
    let clear_key = gtk::Button::with_label("Clear key");
    let models = gtk::Button::with_label("List models");
    save.set_widget_name("save-provider");
    save_key.set_widget_name("save-key");
    clear_key.set_widget_name("clear-key");
    models.set_widget_name("list-models");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.append(&save);
    buttons.append(&save_key);
    buttons.append(&clear_key);
    buttons.append(&models);
    root.append(&grid);
    root.append(&note);
    root.append(&secret);
    root.append(&secret_status);
    root.append(&buttons);
    Form {
        root,
        id,
        kind,
        endpoint,
        model,
        temperature,
        timeout,
        share,
        auth_env,
        secret,
        secret_status,
    }
}

fn hook_buttons(ctx: &Rc<Ctx>, form: &Form) {
    let ctx_save = Rc::clone(ctx);
    let form_save = clone_form(form);
    named_button(&form.root, "save-provider")
        .connect_clicked(move |_| save_provider(&ctx_save, &form_save));
    let ctx_key = Rc::clone(ctx);
    let form_key = clone_form(form);
    named_button(&form.root, "save-key").connect_clicked(move |_| save_key(&ctx_key, &form_key));
    let ctx_clear = Rc::clone(ctx);
    let form_clear = clone_form(form);
    named_button(&form.root, "clear-key")
        .connect_clicked(move |_| clear_key(&ctx_clear, &form_clear));
    let ctx_models = Rc::clone(ctx);
    let form_models = clone_form(form);
    named_button(&form.root, "list-models").connect_clicked(move |_| {
        ctx_models.command(Command::ListModels {
            id: form_models.id.text().to_string(),
        });
    });
}

fn clone_form(form: &Form) -> Form {
    Form {
        root: form.root.clone(),
        id: form.id.clone(),
        kind: form.kind.clone(),
        endpoint: form.endpoint.clone(),
        model: form.model.clone(),
        temperature: form.temperature.clone(),
        timeout: form.timeout.clone(),
        share: form.share.clone(),
        auth_env: form.auth_env.clone(),
        secret: form.secret.clone(),
        secret_status: form.secret_status.clone(),
    }
}

fn save_provider(ctx: &Ctx, form: &Form) {
    let id = form.id.text().trim().to_string();
    let provider = ProviderConfig {
        id: id.clone(),
        kind: selected_kind(form.kind.selected()),
        endpoint: form.endpoint.text().trim().to_string(),
        model: form.model.text().trim().to_string(),
        temperature: Some(form.temperature.value() as f32),
        timeout_secs: form.timeout.value() as u64,
        context_messages: preserved_messages(ctx, &id),
        context_window: preserved_window(ctx, &id),
        share_system_context: form.share.is_active(),
        tool_calling: preserved_tools(ctx, &id),
        auth_env: blank_to_none(&form.auth_env.text()),
    };
    let json = match serde_json::to_string(&provider) {
        Ok(json) => json,
        Err(err) => {
            ctx.widgets.status.set_text(&err.to_string());
            return;
        }
    };
    ctx.model.borrow_mut().provider_id = id;
    ctx.command(Command::UpsertProvider { json });
}

fn save_key(ctx: &Ctx, form: &Form) {
    let secret = form.secret.text().to_string();
    if secret.trim().is_empty() {
        ctx.widgets
            .status
            .set_text("Type a key first. It is not shown again.");
        return;
    }
    let id = form.id.text().trim().to_string();
    form.secret.set_text("");
    ctx.command(Command::SetSecret {
        id: id.clone(),
        secret,
    });
    ctx.command(Command::SecretStatus { id });
}

fn clear_key(ctx: &Ctx, form: &Form) {
    let id = form.id.text().trim().to_string();
    form.secret.set_text("");
    ctx.command(Command::ClearSecret { id: id.clone() });
    ctx.command(Command::SecretStatus { id });
}

fn preserved_messages(ctx: &Ctx, id: &str) -> usize {
    ctx.model
        .borrow()
        .providers
        .iter()
        .find(|provider| provider.id == id)
        .map(|provider| provider.context_messages)
        .unwrap_or(20)
}

fn preserved_window(ctx: &Ctx, id: &str) -> Option<u32> {
    ctx.model
        .borrow()
        .providers
        .iter()
        .find(|provider| provider.id == id)
        .and_then(|provider| provider.context_window)
        .or(Some(8192))
}

fn preserved_tools(ctx: &Ctx, id: &str) -> ToolCallingMode {
    ctx.model
        .borrow()
        .providers
        .iter()
        .find(|provider| provider.id == id)
        .map(|provider| provider.tool_calling)
        .unwrap_or_default()
}

fn blank_to_none(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn selected_kind(index: u32) -> ProviderKind {
    if index == 1 {
        return ProviderKind::OpenaiCompatible;
    }
    ProviderKind::Ollama
}

fn kind_index(kind: ProviderKind) -> u32 {
    match kind {
        ProviderKind::Ollama => 0,
        ProviderKind::OpenaiCompatible => 1,
    }
}

fn hook_close(ctx: &Rc<Ctx>, dialog: &gtk::Window) {
    let ctx = Rc::clone(ctx);
    dialog.connect_close_request(move |_dialog| {
        ctx.model.borrow_mut().secret_label = None;
        glib::Propagation::Proceed
    });
}

fn named_button(root: &impl IsA<gtk::Widget>, name: &str) -> gtk::Button {
    find_button(root.as_ref(), name).unwrap_or_else(|| gtk::Button::with_label(name))
}

fn find_button(widget: &gtk::Widget, name: &str) -> Option<gtk::Button> {
    if widget.widget_name() == name {
        return widget.clone().downcast().ok();
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = find_button(&current, name) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

fn attach(grid: &gtk::Grid, row: i32, label: &str, widget: &impl IsA<gtk::Widget>) {
    let name = gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .build();
    grid.attach(&name, 0, row, 1, 1);
    grid.attach(widget, 1, row, 1, 1);
    widget.set_hexpand(true);
}

fn entry(text: &str) -> gtk::Entry {
    gtk::Entry::builder().text(text).hexpand(true).build()
}
