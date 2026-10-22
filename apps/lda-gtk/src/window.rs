//! Conversation window. Widgets are touched only on the GTK thread.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk::{glib, prelude::*};
use lda_common::{ConversationSummary, ProviderConfig, StreamEvent};
use lda_visual::{Connection, DaemonVisualEvent, EntityCallbacks, EntityEvent, EntityWidget};

use crate::bus::{Command, UiMsg};
use crate::markdown::render_transcript;

pub struct Widgets {
    pub window: gtk::ApplicationWindow,
    pub entity_window: gtk::ApplicationWindow,
    pub entity: EntityWidget,
    pub providers: gtk::DropDown,
    pub conversations: gtk::ListBox,
    pub search: gtk::SearchEntry,
    pub transcript: gtk::TextView,
    pub live: gtk::Label,
    pub input: gtk::TextView,
    pub status: gtk::Label,
}

pub struct Model {
    pub conversation_id: Option<String>,
    pub request_id: Option<String>,
    pub provider_id: String,
    pub providers: Vec<ProviderConfig>,
    pub busy: bool,
    pub connected: bool,
    pub search_generation: u64,
    pub pending: Vec<Pending>,
    pub secret_label: Option<gtk::Label>,
}

pub enum Pending {
    Delta {
        request_id: String,
        text: String,
    },
    Event {
        request_id: String,
        event: StreamEvent,
    },
    Done {
        request_id: String,
        status: String,
        error: String,
    },
}

pub struct Ctx {
    pub cmd: async_channel::Sender<Command>,
    pub widgets: Widgets,
    pub model: RefCell<Model>,
}

impl Ctx {
    pub fn command(&self, command: Command) {
        if self.cmd.try_send(command).is_err() {
            set_status(&self.widgets, "The background connection is closed.");
        }
    }
}

pub fn build(app: &gtk::Application, cmd: async_channel::Sender<Command>) -> Rc<Ctx> {
    let widgets = build_widgets(app);
    let ctx = Rc::new(Ctx {
        cmd,
        widgets,
        model: RefCell::new(Model {
            conversation_id: None,
            request_id: None,
            provider_id: "ollama".to_string(),
            providers: Vec::new(),
            busy: false,
            connected: false,
            search_generation: 0,
            pending: Vec::new(),
            secret_label: None,
        }),
    });
    hook_actions(&ctx);
    install_css();
    ctx.widgets.entity_window.present();
    ctx.widgets.window.present();
    ctx.widgets.input.grab_focus();
    ctx
}

pub fn bind_events(ctx: Rc<Ctx>, rx: async_channel::Receiver<UiMsg>) {
    glib::spawn_future_local(async move {
        while let Ok(msg) = rx.recv().await {
            on_ui(&ctx, msg);
        }
    });
}

fn build_widgets(app: &gtk::Application) -> Widgets {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Linux Desktop Agent")
        .default_width(980)
        .default_height(680)
        .build();
    let header = gtk::HeaderBar::new();
    let new_button = named_button("New", "new");
    let delete_button = named_button("Delete", "delete");
    let stop_button = named_button("Stop", "stop");
    let copy_button = named_button("Copy", "copy");
    let settings_button = named_button("Settings", "settings");
    let widget_button = named_button("Widget", "widget");
    let quit_daemon = named_button("Quit daemon", "quit-daemon");
    let providers = gtk::DropDown::from_strings(&["ollama"]);
    providers.set_tooltip_text(Some("Provider"));
    header.pack_start(&new_button);
    header.pack_start(&delete_button);
    header.pack_end(&quit_daemon);
    header.pack_end(&settings_button);
    header.pack_end(&widget_button);
    header.pack_end(&copy_button);
    header.pack_end(&stop_button);
    header.pack_end(&providers);
    window.set_titlebar(Some(&header));

    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search conversations")
        .build();
    let conversations = gtk::ListBox::new();
    conversations.set_selection_mode(gtk::SelectionMode::Browse);
    let side_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    side_box.set_margin_start(6);
    side_box.set_margin_end(6);
    side_box.set_margin_top(6);
    side_box.set_margin_bottom(6);
    side_box.append(&search);
    let delete_all = named_button("Delete all", "delete-all");
    side_box.append(&delete_all);
    let side_scroll = scrolled(&conversations, true);
    side_box.append(&side_scroll);
    side_scroll.set_vexpand(true);
    side_box.set_size_request(240, -1);

    let transcript = text_view(false);
    transcript.add_css_class("lda-transcript");
    let transcript_scroll = scrolled(&transcript, true);
    transcript_scroll.set_vexpand(true);
    let live = gtk::Label::builder()
        .wrap(true)
        .selectable(true)
        .xalign(0.0)
        .build();
    live.add_css_class("lda-live");
    let live_scroll = scrolled(&live, false);
    live_scroll.set_size_request(-1, 72);
    let input = text_view(true);
    input.set_accepts_tab(false);
    input.set_tooltip_text(Some("Message. Enter sends. Shift+Enter inserts a newline."));
    let input_scroll = scrolled(&input, false);
    input_scroll.set_size_request(-1, 88);
    let send_button = named_button("Send", "send");
    let status = gtk::Label::builder()
        .label("Connecting…")
        .xalign(0.0)
        .wrap(true)
        .build();
    status.add_css_class("lda-status");
    let input_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    input_row.append(&input_scroll);
    input_row.append(&send_button);
    input_scroll.set_hexpand(true);
    let chat = gtk::Box::new(gtk::Orientation::Vertical, 6);
    chat.set_margin_start(8);
    chat.set_margin_end(8);
    chat.set_margin_top(8);
    chat.set_margin_bottom(8);
    chat.append(&transcript_scroll);
    chat.append(&live_scroll);
    chat.append(&input_row);
    chat.append(&status);

    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_start_child(Some(&side_box));
    paned.set_end_child(Some(&chat));
    paned.set_resize_start_child(false);
    paned.set_shrink_start_child(false);
    paned.set_position(260);
    window.set_child(Some(&paned));

    let entity = EntityWidget::new(176);
    let entity_window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Prometeus")
        .decorated(false)
        .resizable(false)
        .default_width(176)
        .default_height(176)
        .child(entity.widget())
        .build();
    entity_window.add_css_class("prometeus-entity-window");
    entity_window.set_hide_on_close(true);

    Widgets {
        window,
        entity_window,
        entity,
        providers,
        conversations,
        search,
        transcript,
        live,
        input,
        status,
    }
}

fn hook_actions(ctx: &Rc<Ctx>) {
    connect_clicked(ctx, "new", |ctx| new_conversation(ctx));
    connect_clicked(ctx, "delete", confirm_delete);
    connect_clicked(ctx, "delete-all", confirm_delete_all);
    connect_clicked(ctx, "stop", |ctx| stop(ctx));
    connect_clicked(ctx, "copy", |ctx| copy_transcript(&ctx.widgets.transcript));
    connect_clicked(ctx, "settings", crate::settings::open);
    connect_clicked(ctx, "widget", toggle_entity);
    connect_clicked(ctx, "quit-daemon", confirm_quit_daemon);
    connect_clicked(ctx, "send", |ctx| send(ctx));
    hook_input(ctx);
    hook_search(ctx);
    hook_rows(ctx);
    hook_providers(ctx);
    hook_entity(ctx);
}

fn connect_clicked(ctx: &Rc<Ctx>, name: &str, action: fn(&Rc<Ctx>)) {
    let window = &ctx.widgets.window;
    let from_title = window.titlebar().and_then(|bar| find_named(&bar, name));
    let from_child = window.child().and_then(|child| find_named(&child, name));
    let Some(button) = from_title.or(from_child) else {
        return;
    };
    let ctx = Rc::clone(ctx);
    button.connect_clicked(move |_| action(&ctx));
}

fn find_named(widget: &gtk::Widget, name: &str) -> Option<gtk::Button> {
    if widget.widget_name() == name {
        return widget.clone().downcast::<gtk::Button>().ok();
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = find_named(&current, name) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

fn hook_input(ctx: &Rc<Ctx>) {
    let controller = gtk::EventControllerKey::new();
    let ctx_key = Rc::clone(ctx);
    controller.connect_key_pressed(move |_controller, key, _code, modifiers| {
        on_input_key(&ctx_key, key, modifiers)
    });
    ctx.widgets.input.add_controller(controller);
}

fn on_input_key(
    ctx: &Ctx,
    key: gtk::gdk::Key,
    modifiers: gtk::gdk::ModifierType,
) -> glib::Propagation {
    let enter = key == gtk::gdk::Key::Return || key == gtk::gdk::Key::KP_Enter;
    if !enter || modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
        return glib::Propagation::Proceed;
    }
    send(ctx);
    glib::Propagation::Stop
}

fn hook_search(ctx: &Rc<Ctx>) {
    let ctx_changed = Rc::clone(ctx);
    ctx.widgets.search.connect_search_changed(move |_| {
        let generation = bump_search(&ctx_changed);
        schedule_search(&ctx_changed, generation);
    });
}

fn bump_search(ctx: &Ctx) -> u64 {
    let mut model = ctx.model.borrow_mut();
    model.search_generation = model.search_generation.wrapping_add(1);
    model.search_generation
}

fn schedule_search(ctx: &Rc<Ctx>, generation: u64) {
    let ctx = Rc::clone(ctx);
    glib::timeout_add_local(Duration::from_millis(200), move || {
        run_search(&ctx, generation)
    });
}

fn run_search(ctx: &Ctx, generation: u64) -> glib::ControlFlow {
    if ctx.model.borrow().search_generation != generation {
        return glib::ControlFlow::Break;
    }
    let query = ctx.widgets.search.text().to_string();
    ctx.command(Command::ListConversations { query });
    glib::ControlFlow::Break
}

fn hook_rows(ctx: &Rc<Ctx>) {
    let ctx_row = Rc::clone(ctx);
    ctx.widgets
        .conversations
        .connect_row_activated(move |_list, row| open_row(&ctx_row, row));
}

fn open_row(ctx: &Ctx, row: &gtk::ListBoxRow) {
    if ctx.model.borrow().busy {
        set_status(
            &ctx.widgets,
            "Stop the current reply before switching conversations.",
        );
        return;
    }
    let id = row.widget_name().to_string();
    ctx.model.borrow_mut().conversation_id = Some(id.clone());
    ctx.command(Command::GetMessages {
        conversation_id: id,
    });
}

fn hook_providers(ctx: &Rc<Ctx>) {
    let ctx_drop = Rc::clone(ctx);
    ctx.widgets
        .providers
        .connect_selected_notify(move |dropdown| {
            let id = selected_provider(dropdown);
            if !id.is_empty() {
                ctx_drop.model.borrow_mut().provider_id = id;
            }
        });
}

fn hook_entity(ctx: &Rc<Ctx>) {
    let activate = Rc::downgrade(ctx);
    let context = Rc::downgrade(ctx);
    ctx.widgets.entity.set_callbacks(EntityCallbacks {
        activate: Some(Box::new(move || {
            if let Some(ctx) = activate.upgrade() {
                show_conversation(&ctx);
            }
        })),
        context_menu: Some(Box::new(move |x, y| {
            if let Some(ctx) = context.upgrade() {
                show_entity_menu(&ctx, x, y);
            }
        })),
        ..EntityCallbacks::default()
    });
}

fn toggle_entity(ctx: &Rc<Ctx>) {
    if ctx.widgets.entity_window.is_visible() {
        ctx.widgets.entity_window.set_visible(false);
        ctx.widgets.entity.dispatch(EntityEvent::SetVisible(false));
    } else {
        ctx.widgets.entity_window.present();
        ctx.widgets.entity.dispatch(EntityEvent::SetVisible(true));
    }
}

fn show_conversation(ctx: &Ctx) {
    ctx.widgets.window.present();
    ctx.widgets.input.grab_focus();
}

fn show_entity_menu(ctx: &Rc<Ctx>, x: f64, y: f64) {
    let popover = gtk::Popover::new();
    popover.set_parent(ctx.widgets.entity.widget());
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.set_has_arrow(true);

    let items = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let open = button("Open Prometeus");
    let hide = button("Hide widget");
    let settings = button("Settings");
    let quit = button("Quit Prometeus");
    items.append(&open);
    items.append(&hide);
    items.append(&settings);
    items.append(&quit);
    popover.set_child(Some(&items));

    let weak = Rc::downgrade(ctx);
    open.connect_clicked(move |_| {
        if let Some(ctx) = weak.upgrade() {
            show_conversation(&ctx);
        }
    });
    let weak = Rc::downgrade(ctx);
    hide.connect_clicked(move |_| {
        if let Some(ctx) = weak.upgrade() {
            ctx.widgets.entity_window.set_visible(false);
            ctx.widgets.entity.dispatch(EntityEvent::SetVisible(false));
        }
    });
    let weak = Rc::downgrade(ctx);
    settings.connect_clicked(move |_| {
        if let Some(ctx) = weak.upgrade() {
            crate::settings::open(&ctx);
        }
    });
    let weak = Rc::downgrade(ctx);
    quit.connect_clicked(move |_| {
        if let Some(ctx) = weak.upgrade() {
            if let Some(application) = ctx.widgets.window.application() {
                application.quit();
            }
        }
    });
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}

fn new_conversation(ctx: &Ctx) {
    if ctx.model.borrow().busy {
        set_status(&ctx.widgets, "Stop the current reply first.");
        return;
    }
    ctx.model.borrow_mut().conversation_id = None;
    ctx.model.borrow_mut().request_id = None;
    ctx.widgets.transcript.buffer().set_text("");
    ctx.widgets.live.set_text("");
    set_status(&ctx.widgets, "New conversation.");
}

fn send(ctx: &Ctx) {
    if ctx.model.borrow().busy {
        set_status(&ctx.widgets, "A reply is still running.");
        return;
    }
    let text = buffer_text(&ctx.widgets.input.buffer());
    if text.trim().is_empty() {
        return;
    }
    ctx.widgets.input.buffer().set_text("");
    let provider_id = ctx.model.borrow().provider_id.clone();
    let conversation_id = ctx
        .model
        .borrow()
        .conversation_id
        .clone()
        .unwrap_or_default();
    {
        let mut model = ctx.model.borrow_mut();
        model.busy = true;
        model.pending.clear();
    }
    ctx.widgets.live.set_text("");
    ctx.widgets.entity.dispatch(EntityEvent::RequestStarted);
    set_status(&ctx.widgets, "Sending…");
    ctx.command(Command::Send {
        conversation_id,
        provider_id,
        text,
    });
}

fn stop(ctx: &Ctx) {
    let Some(request_id) = ctx.model.borrow().request_id.clone() else {
        set_status(&ctx.widgets, "Nothing is running.");
        return;
    };
    ctx.command(Command::Cancel { request_id });
    set_status(&ctx.widgets, "Stopping…");
}

fn confirm_delete(ctx: &Rc<Ctx>) {
    let Some(id) = ctx.model.borrow().conversation_id.clone() else {
        set_status(&ctx.widgets, "Select a conversation first.");
        return;
    };
    confirm(
        ctx,
        "Delete this conversation?",
        "The messages are removed from this computer.",
        move |ctx| {
            ctx.command(Command::DeleteConversation { id });
        },
    );
}

fn confirm_delete_all(ctx: &Rc<Ctx>) {
    confirm(
        ctx,
        "Delete every conversation?",
        "This removes the local history. Provider settings stay.",
        |ctx| ctx.command(Command::DeleteAll),
    );
}

fn confirm_quit_daemon(ctx: &Rc<Ctx>) {
    confirm(
        ctx,
        "Stop the background daemon?",
        "The window stays open. Start the app again to reconnect.",
        |ctx| {
            ctx.command(Command::Shutdown);
        },
    );
}

fn confirm(ctx: &Rc<Ctx>, message: &str, detail: &str, action: impl FnOnce(&Ctx) + 'static) {
    let dialog = gtk::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .buttons(["Cancel", "Continue"])
        .cancel_button(0)
        .default_button(0)
        .modal(true)
        .build();
    let ctx = Rc::clone(ctx);
    let parent = ctx.widgets.window.clone();
    dialog.choose(
        Some(&parent),
        None::<&gtk::gio::Cancellable>,
        move |response| {
            if response == Ok(1) {
                action(&ctx);
            }
        },
    );
}

fn on_ui(ctx: &Ctx, msg: UiMsg) {
    match msg {
        UiMsg::Connected => on_connected(ctx),
        UiMsg::Status(text) => {
            if !ctx.model.borrow().connected {
                ctx.widgets
                    .entity
                    .apply_daemon_event(DaemonVisualEvent::Disconnected);
            }
            set_status(&ctx.widgets, &text);
        }
        UiMsg::RequestFailed(text) => on_failed(ctx, &text),
        UiMsg::SendStarted {
            request_id,
            conversation_id,
        } => on_started(ctx, request_id, conversation_id),
        UiMsg::Delta { request_id, text } => on_delta(ctx, request_id, text),
        UiMsg::Event { request_id, event } => on_event(ctx, request_id, event),
        UiMsg::Done {
            request_id,
            status,
            error,
        } => on_done(ctx, &request_id, &status, &error),
        UiMsg::Conversations(rows) => refill_conversations(ctx, &rows),
        UiMsg::Messages {
            conversation_id,
            messages,
        } => show_messages(ctx, &conversation_id, &messages),
        UiMsg::Providers(providers) => on_providers(ctx, providers),
        UiMsg::Models(models) => on_models(ctx, &models),
        UiMsg::SecretStatus(status) => on_secret_status(ctx, &status),
        UiMsg::Deleted { id } => on_deleted(ctx, &id),
        UiMsg::DeletedAll => on_deleted_all(ctx),
        UiMsg::ShutdownDone => on_shutdown(ctx),
    }
}

fn on_connected(ctx: &Ctx) {
    ctx.model.borrow_mut().connected = true;
    ctx.widgets
        .entity
        .apply_daemon_event(DaemonVisualEvent::Connected);
    ctx.widgets.entity.dispatch(EntityEvent::Initialized);
    set_status(
        &ctx.widgets,
        "Connected. Enter sends. Shift+Enter inserts a newline.",
    );
    ctx.command(Command::ListProviders);
    ctx.command(Command::ListConversations {
        query: String::new(),
    });
}

fn on_failed(ctx: &Ctx, text: &str) {
    ctx.model.borrow_mut().busy = false;
    ctx.widgets
        .entity
        .dispatch(EntityEvent::RequestFailed { recoverable: true });
    set_status(&ctx.widgets, text);
}

fn on_started(ctx: &Ctx, request_id: String, conversation_id: String) {
    ctx.widgets
        .entity
        .apply_daemon_event(DaemonVisualEvent::RequestAccepted);
    let pending = {
        let mut model = ctx.model.borrow_mut();
        model.request_id = Some(request_id);
        model.conversation_id = Some(conversation_id.clone());
        model.busy = true;
        std::mem::take(&mut model.pending)
    };
    for item in pending {
        apply_pending(ctx, item);
    }
    let query = ctx.widgets.search.text().to_string();
    ctx.command(Command::ListConversations { query });
    ctx.command(Command::GetMessages { conversation_id });
    set_status(&ctx.widgets, "Receiving…");
}

fn apply_pending(ctx: &Ctx, item: Pending) {
    match item {
        Pending::Delta { request_id, text } => on_delta(ctx, request_id, text),
        Pending::Event { request_id, event } => on_event(ctx, request_id, event),
        Pending::Done {
            request_id,
            status,
            error,
        } => on_done(ctx, &request_id, &status, &error),
    }
}

fn on_delta(ctx: &Ctx, request_id: String, text: String) {
    if active_request(ctx, &request_id) {
        ctx.widgets
            .entity
            .apply_daemon_event(DaemonVisualEvent::Delta);
        append_live(&ctx.widgets.live, &text);
        return;
    }
    if waiting_for_id(ctx) {
        ctx.model
            .borrow_mut()
            .pending
            .push(Pending::Delta { request_id, text });
    }
}

fn on_event(ctx: &Ctx, request_id: String, event: StreamEvent) {
    if !active_request(ctx, &request_id) {
        if waiting_for_id(ctx) {
            ctx.model
                .borrow_mut()
                .pending
                .push(Pending::Event { request_id, event });
        }
        return;
    }
    ctx.widgets
        .entity
        .apply_daemon_event(DaemonVisualEvent::Event(&event));
    match &event {
        StreamEvent::ToolStarted { name, .. } => {
            append_live(&ctx.widgets.live, &format!("\nRunning {name}…"))
        }
        StreamEvent::ToolFinished { name, ok, .. } => {
            append_live(&ctx.widgets.live, &format!("\nFinished {name} (ok={ok})"));
        }
        StreamEvent::ReplaceText { text } => ctx.widgets.live.set_text(text),
    }
}

fn on_done(ctx: &Ctx, request_id: &str, status: &str, error: &str) {
    if !active_request(ctx, request_id) {
        if waiting_for_id(ctx) {
            ctx.model.borrow_mut().pending.push(Pending::Done {
                request_id: request_id.to_string(),
                status: status.to_string(),
                error: error.to_string(),
            });
        }
        return;
    }
    ctx.widgets
        .entity
        .apply_daemon_event(DaemonVisualEvent::Done { status, error });
    ctx.model.borrow_mut().busy = false;
    ctx.model.borrow_mut().request_id = None;
    ctx.widgets.live.set_text("");
    if let Some(id) = ctx.model.borrow().conversation_id.clone() {
        ctx.command(Command::GetMessages {
            conversation_id: id,
        });
        ctx.command(Command::ListConversations {
            query: ctx.widgets.search.text().to_string(),
        });
    }
    set_status(&ctx.widgets, &done_note(status, error));
}

fn done_note(status: &str, error: &str) -> String {
    match status {
        "ok" => "Ready.".to_string(),
        "cancelled" => "Stopped.".to_string(),
        _ if error.is_empty() => "The request failed.".to_string(),
        _ => format!("Error: {error}"),
    }
}

fn show_messages(ctx: &Ctx, conversation_id: &str, messages: &[lda_common::StoredMessage]) {
    let current = ctx.model.borrow().conversation_id.clone();
    if current.as_deref() != Some(conversation_id) {
        return;
    }
    render_transcript(&ctx.widgets.transcript.buffer(), messages);
    scroll_to_end(&ctx.widgets.transcript);
}

fn on_providers(ctx: &Ctx, providers: Vec<ProviderConfig>) {
    let current = ctx.model.borrow().provider_id.clone();
    let ids: Vec<String> = providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect();
    let keep = if ids.iter().any(|id| id == &current) {
        current
    } else {
        ids.first().cloned().unwrap_or_default()
    };
    set_provider_list(&ctx.widgets.providers, &ids, &keep);
    let mut model = ctx.model.borrow_mut();
    model.providers = providers;
    model.provider_id = keep;
}

fn on_models(ctx: &Ctx, models: &[String]) {
    if models.is_empty() {
        set_status(&ctx.widgets, "No models reported.");
        return;
    }
    set_status(&ctx.widgets, &format!("Models: {}", models.join(", ")));
}

fn on_secret_status(ctx: &Ctx, status: &str) {
    let text = match status {
        "store" => "Key: stored in the secret service.",
        "env" => "Key: an environment variable is set.",
        _ => "Key: not set.",
    };
    let label = ctx.model.borrow().secret_label.clone();
    if let Some(label) = label {
        label.set_text(text);
    }
}

fn on_deleted(ctx: &Ctx, id: &str) {
    let current = ctx.model.borrow().conversation_id.clone();
    if current.as_deref() == Some(id) {
        ctx.model.borrow_mut().conversation_id = None;
        ctx.widgets.transcript.buffer().set_text("");
    }
    ctx.command(Command::ListConversations {
        query: ctx.widgets.search.text().to_string(),
    });
    set_status(&ctx.widgets, "Conversation deleted.");
}

fn on_deleted_all(ctx: &Ctx) {
    ctx.model.borrow_mut().conversation_id = None;
    ctx.widgets.transcript.buffer().set_text("");
    ctx.command(Command::ListConversations {
        query: String::new(),
    });
    set_status(&ctx.widgets, "All conversations deleted.");
}

fn on_shutdown(ctx: &Ctx) {
    ctx.model.borrow_mut().busy = false;
    ctx.model.borrow_mut().connected = false;
    ctx.widgets
        .entity
        .dispatch(EntityEvent::SetConnection(Connection::Offline));
    set_status(
        &ctx.widgets,
        "Daemon stopped. Start the app again to reconnect.",
    );
}

fn refill_conversations(ctx: &Ctx, items: &[ConversationSummary]) {
    let list = &ctx.widgets.conversations;
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let current = ctx.model.borrow().conversation_id.clone();
    for item in items {
        let row = conversation_row(item);
        list.append(&row);
        if current.as_deref() == Some(item.id.as_str()) {
            list.select_row(Some(&row));
        }
    }
}

fn conversation_row(item: &ConversationSummary) -> gtk::ListBoxRow {
    let label = gtk::Label::builder()
        .label(&item.title)
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    label.set_margin_start(8);
    label.set_margin_end(8);
    label.set_margin_top(6);
    label.set_margin_bottom(6);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&label));
    row.set_widget_name(&item.id);
    row
}

fn set_provider_list(dropdown: &gtk::DropDown, ids: &[String], current: &str) {
    if ids.is_empty() {
        return;
    }
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    dropdown.set_model(Some(&gtk::StringList::new(&refs)));
    if let Some(index) = ids.iter().position(|id| id == current) {
        dropdown.set_selected(index as u32);
    }
}

fn selected_provider(dropdown: &gtk::DropDown) -> String {
    let Some(model) = dropdown.model() else {
        return String::new();
    };
    let Some(item) = model.item(dropdown.selected()) else {
        return String::new();
    };
    item.downcast::<gtk::StringObject>()
        .map(|object| object.string().to_string())
        .unwrap_or_default()
}

fn active_request(ctx: &Ctx, request_id: &str) -> bool {
    ctx.model.borrow().request_id.as_deref() == Some(request_id)
}

fn waiting_for_id(ctx: &Ctx) -> bool {
    let model = ctx.model.borrow();
    model.busy && model.request_id.is_none()
}

fn append_live(label: &gtk::Label, extra: &str) {
    label.set_text(&capped(&label.text(), extra));
}

fn capped(current: &str, extra: &str) -> String {
    let mut text = String::new();
    text.push_str(current);
    text.push_str(extra);
    let count = text.chars().count();
    if count <= 64_000 {
        return text;
    }
    let skip = count - 64_000;
    let mut out = String::from("…\n");
    out.extend(text.chars().skip(skip));
    out
}

fn buffer_text(buffer: &gtk::TextBuffer) -> String {
    let (start, end) = buffer.bounds();
    buffer.text(&start, &end, false).to_string()
}

fn copy_transcript(view: &gtk::TextView) {
    let buffer = view.buffer();
    let text = if let Some((start, end)) = buffer.selection_bounds() {
        buffer.text(&start, &end, false).to_string()
    } else {
        buffer_text(&buffer)
    };
    view.clipboard().set_text(&text);
}

fn scroll_to_end(view: &gtk::TextView) {
    let buffer = view.buffer();
    let mut end = buffer.end_iter();
    view.scroll_to_iter(&mut end, 0.0, true, 0.0, 1.0);
}

fn set_status(widgets: &Widgets, text: &str) {
    widgets.status.set_text(text);
}

fn button(label: &str) -> gtk::Button {
    gtk::Button::with_label(label)
}

fn named_button(label: &str, name: &str) -> gtk::Button {
    let button = button(label);
    button.set_widget_name(name);
    button
}

fn text_view(editable: bool) -> gtk::TextView {
    let view = gtk::TextView::builder()
        .editable(editable)
        .wrap_mode(gtk::WrapMode::WordChar)
        .cursor_visible(true)
        .build();
    view.set_left_margin(8);
    view.set_right_margin(8);
    view.set_top_margin(6);
    view.set_bottom_margin(6);
    view
}

fn scrolled(child: &impl IsA<gtk::Widget>, expand: bool) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .hexpand(true)
        .vexpand(expand)
        .build()
}

fn install_css() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

const CSS: &str = "
textview.lda-transcript { font-size: 1.05em; }
label.lda-live { font-size: 1.02em; }
label.lda-status { opacity: 0.75; }
window.prometeus-entity-window { background: transparent; }
";
