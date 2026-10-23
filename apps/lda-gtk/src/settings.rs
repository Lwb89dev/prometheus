//! Provider editor. Secrets go to the daemon's secret-service call and are not written
//! into the configuration file. The entry is cleared after a successful hand-off.
//!
//! This window is libadwaita. It does not link into the daemon.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use lda_common::{ProviderConfig, ProviderKind, ToolCallingMode};

use crate::bus::Command;
use crate::window::Ctx;

struct Form {
    group: adw::PreferencesGroup,
    id: adw::EntryRow,
    kind: adw::ComboRow,
    endpoint: adw::EntryRow,
    model: adw::EntryRow,
    temperature: gtk::SpinButton,
    timeout: gtk::SpinButton,
    share: adw::SwitchRow,
    auth_env: adw::EntryRow,
    secret: adw::PasswordEntryRow,
    secret_status: gtk::Label,
}

struct Actions {
    save: gtk::Button,
    save_key: gtk::Button,
    clear_key: gtk::Button,
    models: gtk::Button,
}

pub fn open(ctx: &Rc<Ctx>) {
    let provider = current_provider(ctx);
    let dialog = adw::PreferencesWindow::builder()
        .transient_for(&ctx.widgets.window)
        .modal(true)
        .title("Providers")
        .default_width(560)
        .default_height(640)
        .build();
    let (form, actions) = build_form(&provider);
    let page = adw::PreferencesPage::new();
    page.set_title("Provider");
    page.set_description(
        "Cloud providers are contacted only after you save one here and send a message to it.",
    );
    page.add(&form.group);
    dialog.add(&page);
    ctx.model.borrow_mut().secret_label = Some(form.secret_status.clone());
    hook_close(ctx, &dialog);
    hook_buttons(ctx, &form, &actions);
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

fn build_form(provider: &ProviderConfig) -> (Form, Actions) {
    let group = adw::PreferencesGroup::new();
    group.set_title("Connection");
    group.set_description(Some(
        "Loopback providers can use system tools without the switch. For any other endpoint, leave it off unless you want host details to leave this computer. API keys are not saved in the configuration file.",
    ));
    let id = adw::EntryRow::builder()
        .title("Id")
        .text(&provider.id)
        .build();
    let kinds = gtk::StringList::new(&["ollama", "openai_compatible", "anthropic", "xai"]);
    let kind = adw::ComboRow::builder()
        .title("Kind")
        .model(&kinds)
        .selected(kind_index(provider.kind))
        .build();
    let endpoint = adw::EntryRow::builder()
        .title("Endpoint")
        .text(&provider.endpoint)
        .build();
    let model = adw::EntryRow::builder()
        .title("Model")
        .text(&provider.model)
        .build();
    let temperature = gtk::SpinButton::with_range(0.0, 2.0, 0.1);
    temperature.set_digits(2);
    temperature.set_value(f64::from(provider.temperature.unwrap_or(0.2)));
    temperature.set_valign(gtk::Align::Center);
    let temperature_row = adw::ActionRow::builder().title("Temperature").build();
    temperature_row.add_suffix(&temperature);
    let timeout = gtk::SpinButton::with_range(1.0, 3600.0, 1.0);
    timeout.set_value(provider.timeout_secs as f64);
    timeout.set_valign(gtk::Align::Center);
    let timeout_row = adw::ActionRow::builder().title("Timeout (seconds)").build();
    timeout_row.add_suffix(&timeout);
    let share = adw::SwitchRow::builder()
        .title("Send system tool results to this provider")
        .active(provider.share_system_context)
        .build();
    let auth_env = adw::EntryRow::builder()
        .title("Auth env var")
        .text(provider.auth_env.as_deref().unwrap_or(""))
        .build();
    let secret = adw::PasswordEntryRow::builder().title("API key").build();
    let secret_status = gtk::Label::builder().label("Key: …").xalign(0.0).build();
    let status_row = adw::ActionRow::new();
    status_row.add_suffix(&secret_status);
    let save = gtk::Button::with_label("Save provider");
    let save_key = gtk::Button::with_label("Save key");
    let clear_key = gtk::Button::with_label("Clear key");
    let models = gtk::Button::with_label("List models");
    save.set_widget_name("save-provider");
    save_key.set_widget_name("save-key");
    clear_key.set_widget_name("clear-key");
    models.set_widget_name("list-models");
    for button in [&save, &save_key, &clear_key, &models] {
        button.set_valign(gtk::Align::Center);
    }
    let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    action_row.set_margin_start(12);
    action_row.set_margin_end(12);
    action_row.set_margin_top(6);
    action_row.set_margin_bottom(6);
    action_row.append(&save);
    action_row.append(&save_key);
    action_row.append(&clear_key);
    action_row.append(&models);
    for row in [
        id.upcast_ref::<gtk::Widget>(),
        kind.upcast_ref(),
        endpoint.upcast_ref(),
        model.upcast_ref(),
        temperature_row.upcast_ref(),
        timeout_row.upcast_ref(),
        share.upcast_ref(),
        auth_env.upcast_ref(),
        secret.upcast_ref(),
        status_row.upcast_ref(),
        action_row.upcast_ref(),
    ] {
        group.add(row);
    }
    let actions = Actions {
        save,
        save_key,
        clear_key,
        models,
    };
    (
        Form {
            group,
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
        },
        actions,
    )
}

fn hook_buttons(ctx: &Rc<Ctx>, form: &Form, actions: &Actions) {
    let ctx_save = Rc::clone(ctx);
    let form_save = clone_form(form);
    actions
        .save
        .connect_clicked(move |_| save_provider(&ctx_save, &form_save));
    let ctx_key = Rc::clone(ctx);
    let form_key = clone_form(form);
    actions
        .save_key
        .connect_clicked(move |_| save_key(&ctx_key, &form_key));
    let ctx_clear = Rc::clone(ctx);
    let form_clear = clone_form(form);
    actions
        .clear_key
        .connect_clicked(move |_| clear_key(&ctx_clear, &form_clear));
    let ctx_models = Rc::clone(ctx);
    let form_models = clone_form(form);
    actions.models.connect_clicked(move |_| {
        ctx_models.command(Command::ListModels {
            id: form_models.id.text().to_string(),
        });
    });
}

fn clone_form(form: &Form) -> Form {
    Form {
        group: form.group.clone(),
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
    match index {
        1 => ProviderKind::OpenaiCompatible,
        2 => ProviderKind::Anthropic,
        3 => ProviderKind::Xai,
        _ => ProviderKind::Ollama,
    }
}

fn kind_index(kind: ProviderKind) -> u32 {
    match kind {
        ProviderKind::Ollama => 0,
        ProviderKind::OpenaiCompatible => 1,
        ProviderKind::Anthropic => 2,
        ProviderKind::Xai => 3,
    }
}

fn hook_close(ctx: &Rc<Ctx>, dialog: &adw::PreferencesWindow) {
    let ctx = Rc::clone(ctx);
    dialog.connect_close_request(move |_dialog| {
        ctx.model.borrow_mut().secret_label = None;
        glib::Propagation::Proceed
    });
}
