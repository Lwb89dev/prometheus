//! Conversation window. Widgets are touched only on the GTK thread.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::accessible::Property;
use gtk::glib;
use lda_common::{
    kind_of, model_kind, ConversationSummary, InstalledModel, ModelKind, ProviderConfig,
    ProviderKind, SourceLink, StreamEvent,
};
use lda_visual::{Connection, DaemonVisualEvent, EntityCallbacks, EntityEvent, EntityWidget};

use crate::bus::{Command, UiMsg};
use crate::markdown::render_transcript;

pub struct Widgets {
    pub window: adw::ApplicationWindow,
    pub entity_window: gtk::ApplicationWindow,
    pub entity: EntityWidget,
    pub providers: gtk::DropDown,
    pub conversations: gtk::ListBox,
    pub search: gtk::SearchEntry,
    pub transcript: gtk::TextView,
    pub sources: gtk::ListBox,
    pub live: gtk::Label,
    pub input: gtk::TextView,
    pub send: gtk::Button,
    pub talk: gtk::ToggleButton,
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
    pub notes_list: Option<gtk::ListBox>,
    pub installed_list: Option<gtk::ListBox>,
    pub suggestion_list: Option<gtk::ListBox>,
    pub setup_notice: Option<gtk::Label>,
    pub setup_dialog: Option<adw::Dialog>,
    pub installed: Vec<InstalledModel>,
    pub draft: String,
    pub talk_generation: u64,
    pub talk_quiet: bool,
    pub outgoing: bool,
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
    pub capture: RefCell<Option<lda_desktop::Capture>>,
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
            notes_list: None,
            installed_list: None,
            suggestion_list: None,
            setup_notice: None,
            setup_dialog: None,
            installed: Vec::new(),
            draft: String::new(),
            talk_generation: 0,
            talk_quiet: false,
            outgoing: false,
        }),
        capture: RefCell::new(None),
    });
    hook_actions(&ctx);
    install_css();
    ctx.widgets.entity_window.present();
    ctx.widgets.window.present();
    ctx.widgets.input.grab_focus();
    // The entity window maps on its own and can become the active toplevel
    // after this call returns. Present the conversation again once that map
    // has settled, or typed keys and Enter never reach the message box.
    focus_chat_later(&ctx);
    ctx
}

fn focus_chat_later(ctx: &Rc<Ctx>) {
    let ctx = Rc::clone(ctx);
    glib::timeout_add_local(Duration::from_millis(150), move || {
        if ctx.model.borrow().setup_dialog.is_some() {
            return glib::ControlFlow::Break;
        }
        ctx.widgets.window.present();
        ctx.widgets.input.grab_focus();
        glib::ControlFlow::Break
    });
}

pub fn bind_events(ctx: Rc<Ctx>, rx: async_channel::Receiver<UiMsg>) {
    glib::spawn_future_local(async move {
        while let Ok(msg) = rx.recv().await {
            on_ui(&ctx, msg);
        }
    });
}

fn build_widgets(app: &gtk::Application) -> Widgets {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Linux Desktop Agent")
        .default_width(980)
        .default_height(680)
        .build();
    let header = adw::HeaderBar::new();
    let new_button = named_button("New", "new");
    let delete_button = named_button("Delete", "delete");
    let stop_button = named_button("Stop", "stop");
    let copy_button = named_button("Copy", "copy");
    let settings_button = named_button("Providers", "settings");
    let models_button = named_button("Models", "models");
    let notes_button = named_button("Notes", "notes");
    let widget_button = named_button("Widget", "widget");
    let quit_daemon = named_button("Quit daemon", "quit-daemon");
    let providers = gtk::DropDown::from_strings(&["ollama"]);
    providers.set_tooltip_text(Some("Provider"));
    header.pack_start(&new_button);
    header.pack_start(&delete_button);
    header.pack_end(&quit_daemon);
    header.pack_end(&settings_button);
    header.pack_end(&models_button);
    header.pack_end(&notes_button);
    header.pack_end(&widget_button);
    header.pack_end(&copy_button);
    header.pack_end(&stop_button);
    header.pack_end(&providers);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);

    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search conversations")
        .build();
    label_widget(&search, "Search conversations");
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
    label_widget(&transcript, "Conversation transcript");
    let transcript_scroll = scrolled(&transcript, true);
    transcript_scroll.set_vexpand(true);
    let sources = gtk::ListBox::new();
    sources.set_selection_mode(gtk::SelectionMode::None);
    sources.add_css_class("lda-sources");
    sources.set_visible(false);
    label_widget(&sources, "Sources");
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
    label_widget(&input, "Message");
    input.set_tooltip_text(Some(
        "Message. Enter sends. Shift+Enter inserts a newline. Talk listens only while it is on.",
    ));
    let input_scroll = scrolled(&input, false);
    input_scroll.set_size_request(-1, 88);
    let send_button = named_button("Send", "send");
    let talk = gtk::ToggleButton::with_label("Talk");
    talk.set_widget_name("talk");
    talk.set_tooltip_text(Some(
        "Talk. Click to open the microphone. Click again to close it. It also closes after 30 seconds. Nothing listens at startup.",
    ));
    label_widget(&talk, "Talk");
    let status = gtk::Label::builder()
        .label("Connecting…")
        .xalign(0.0)
        .wrap(true)
        .build();
    status.add_css_class("lda-status");
    let input_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    input_row.append(&input_scroll);
    input_row.append(&talk);
    input_row.append(&send_button);
    input_scroll.set_hexpand(true);
    let chat = gtk::Box::new(gtk::Orientation::Vertical, 6);
    chat.set_margin_start(8);
    chat.set_margin_end(8);
    chat.set_margin_top(8);
    chat.set_margin_bottom(8);
    chat.append(&transcript_scroll);
    chat.append(&sources);
    chat.append(&live_scroll);
    chat.append(&input_row);
    chat.append(&status);

    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_start_child(Some(&side_box));
    paned.set_end_child(Some(&chat));
    paned.set_resize_start_child(false);
    paned.set_shrink_start_child(false);
    paned.set_position(260);
    toolbar.set_content(Some(&paned));
    window.set_content(Some(&toolbar));

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
        sources,
        live,
        input,
        send: send_button,
        talk,
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
    connect_clicked(ctx, "models", crate::setup::open);
    connect_clicked(ctx, "notes", crate::notes::open);
    connect_clicked(ctx, "widget", toggle_entity);
    connect_clicked(ctx, "quit-daemon", confirm_quit_daemon);
    let ctx_send = Rc::clone(ctx);
    ctx.widgets.send.connect_clicked(move |_| send(&ctx_send));
    crate::talk::hook(ctx);
    hook_input(ctx);
    hook_search(ctx);
    hook_rows(ctx);
    hook_providers(ctx);
    hook_entity(ctx);
}

fn connect_clicked(ctx: &Rc<Ctx>, name: &str, action: fn(&Rc<Ctx>)) {
    let window = &ctx.widgets.window;
    let from_content = window.content().and_then(|child| find_named(&child, name));
    let from_child = window.child().and_then(|child| find_named(&child, name));
    let Some(button) = from_content.or(from_child) else {
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
    // The text view's own key controller is bubble-phase and consumes Return
    // before a later bubble controller runs. Capture runs first.
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let ctx_key = Rc::clone(ctx);
    controller.connect_key_pressed(move |_controller, key, _code, modifiers| {
        on_input_key(&ctx_key, key, modifiers)
    });
    ctx.widgets.input.add_controller(controller);
}

fn on_input_key(
    ctx: &Rc<Ctx>,
    key: gtk::gdk::Key,
    modifiers: gtk::gdk::ModifierType,
) -> glib::Propagation {
    let enter = matches!(
        key,
        gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter | gtk::gdk::Key::ISO_Enter
    );
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
    clear_sources(&ctx.widgets.sources);
    set_status(&ctx.widgets, "New conversation.");
}

fn send(ctx: &Rc<Ctx>) {
    if ctx.model.borrow().busy {
        set_status(&ctx.widgets, "A reply is still running.");
        return;
    }
    if provider_needs_model(ctx) {
        set_status(&ctx.widgets, "Choose a local model first.");
        crate::setup::open(ctx);
        return;
    }
    let text = buffer_text(&ctx.widgets.input.buffer());
    if text.trim().is_empty() {
        set_status(&ctx.widgets, "Write a message, then press Enter.");
        return;
    }
    if crate::talk::route_typed(ctx, &text) {
        return;
    }
    dispatch_chat(ctx, text);
}

pub(crate) fn dispatch_chat(ctx: &Rc<Ctx>, text: String) {
    if ctx.model.borrow().busy {
        set_status(&ctx.widgets, "A reply is still running.");
        return;
    }
    crate::talk::cancel(ctx);
    ctx.widgets.input.buffer().set_text("");
    {
        let mut model = ctx.model.borrow_mut();
        model.draft = text.clone();
        model.outgoing = true;
        model.busy = true;
        model.pending.clear();
    }
    append_outgoing(&ctx.widgets.transcript, &text);
    ctx.widgets.live.set_text("");
    clear_sources(&ctx.widgets.sources);
    ctx.widgets.entity.dispatch(EntityEvent::RequestStarted);
    set_status(&ctx.widgets, "Sending…");
    let provider_id = ctx.model.borrow().provider_id.clone();
    let conversation_id = ctx
        .model
        .borrow()
        .conversation_id
        .clone()
        .unwrap_or_default();
    let command = Command::Send {
        conversation_id,
        provider_id,
        text,
    };
    if ctx.cmd.try_send(command).is_err() {
        set_status(&ctx.widgets, "The background connection is closed.");
        restore_draft(ctx);
        reload_transcript(ctx);
    }
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

fn on_ui(ctx: &Rc<Ctx>, msg: UiMsg) {
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
        UiMsg::Discovery(report) => crate::setup::show(ctx, &report),
        UiMsg::PullProgress {
            phase,
            name,
            detail,
        } => crate::setup::pull_progress(ctx, &phase, &name, &detail),
        UiMsg::SecretStatus(status) => on_secret_status(ctx, &status),
        UiMsg::Deleted { id } => on_deleted(ctx, &id),
        UiMsg::DeletedAll => on_deleted_all(ctx),
        UiMsg::ShutdownDone => on_shutdown(ctx),
        UiMsg::Memories(notes) => crate::notes::show(ctx, &notes),
        UiMsg::Transcript(transcript) => crate::talk::on_transcript(ctx, transcript),
        UiMsg::Speech(wav) => crate::talk::on_speech(ctx, wav),
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
        "Connected. Enter sends. Shift+Enter inserts a newline. Talk listens only while it is on.",
    );
    ctx.command(Command::ListProviders);
    ctx.command(Command::ListConversations {
        query: String::new(),
    });
}

fn on_failed(ctx: &Ctx, text: &str) {
    let outgoing = ctx.model.borrow().outgoing;
    ctx.model.borrow_mut().busy = false;
    ctx.model.borrow_mut().outgoing = false;
    restore_draft(ctx);
    ctx.widgets
        .entity
        .dispatch(EntityEvent::RequestFailed { recoverable: true });
    set_status(&ctx.widgets, text);
    if outgoing {
        reload_transcript(ctx);
    }
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
        model.draft.clear();
        model.outgoing = false;
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
        StreamEvent::Sources { items } => show_sources(&ctx.widgets.sources, items),
        StreamEvent::Confirmation {
            call_id,
            name,
            summary,
        } => {
            set_status(&ctx.widgets, "Waiting for confirmation…");
            ask_confirmation(ctx, request_id, call_id.clone(), name, summary);
        }
    }
}

fn show_sources(list: &gtk::ListBox, items: &[SourceLink]) {
    clear_sources(list);
    let mut shown = 0;
    for item in items {
        if !http_url(&item.url) {
            continue;
        }
        let label = if item.title.trim().is_empty() {
            item.url.clone()
        } else {
            item.title.clone()
        };
        let link = gtk::LinkButton::with_label(&item.url, &label);
        link.set_halign(gtk::Align::Start);
        list.append(&link);
        shown += 1;
    }
    list.set_visible(shown > 0);
}

fn clear_sources(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    list.set_visible(false);
}

fn http_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

fn ask_confirmation(ctx: &Ctx, request_id: String, call_id: String, name: &str, summary: &str) {
    let dialog = gtk::AlertDialog::builder()
        .message(format!("Allow {name}?"))
        .detail(summary)
        .buttons(["Deny", "Allow"])
        .cancel_button(0)
        .default_button(0)
        .modal(true)
        .build();
    let cmd = ctx.cmd.clone();
    let parent = ctx.widgets.window.clone();
    dialog.choose(
        Some(&parent),
        None::<&gtk::gio::Cancellable>,
        move |response| {
            let allow = response == Ok(1);
            let _ = cmd.try_send(Command::Confirm {
                request_id,
                call_id,
                allow,
            });
        },
    );
}

fn label_widget(widget: &impl IsA<gtk::Accessible>, text: &str) {
    widget.update_property(&[Property::Label(text)]);
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

fn on_providers(ctx: &Rc<Ctx>, providers: Vec<ProviderConfig>) {
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
    let needs_model = providers.iter().any(|provider| {
        provider.id == keep
            && provider.kind == ProviderKind::Ollama
            && provider.model.trim().is_empty()
    });
    let discover = providers
        .iter()
        .any(|provider| provider.id == keep && provider.kind == ProviderKind::Ollama);
    let discover_id = keep.clone();
    let mut model = ctx.model.borrow_mut();
    model.providers = providers;
    model.provider_id = keep;
    drop(model);
    if needs_model {
        set_status(
            &ctx.widgets,
            "Choose a local model. Nothing is downloaded until you approve a tag.",
        );
        crate::setup::open(ctx);
        return;
    }
    if discover {
        ctx.command(Command::Discover { id: discover_id });
    }
}

fn provider_needs_model(ctx: &Ctx) -> bool {
    let model = ctx.model.borrow();
    model.providers.iter().any(|provider| {
        provider.id == model.provider_id
            && provider.kind == ProviderKind::Ollama
            && provider.model.trim().is_empty()
    })
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

pub(crate) fn set_status(widgets: &Widgets, text: &str) {
    widgets.status.set_text(text);
}

pub(crate) fn selected_kind(ctx: &Ctx) -> ModelKind {
    let model = ctx.model.borrow();
    let name = model
        .providers
        .iter()
        .find(|provider| provider.id == model.provider_id)
        .map(|provider| provider.model.as_str())
        .unwrap_or("");
    if let Some(found) = model.installed.iter().find(|item| item.name == name) {
        return kind_of(found);
    }
    model_kind(&[], name)
}

pub(crate) fn provider_name(ctx: &Ctx) -> String {
    let model = ctx.model.borrow();
    model
        .providers
        .iter()
        .find(|provider| provider.id == model.provider_id)
        .map(|provider| provider.model.clone())
        .unwrap_or_default()
}

pub(crate) fn current_provider_id(ctx: &Ctx) -> String {
    let id = ctx.model.borrow().provider_id.clone();
    if id.is_empty() {
        return "ollama".to_string();
    }
    id
}

pub(crate) fn restore_draft(ctx: &Ctx) {
    let draft = ctx.model.borrow().draft.clone();
    ctx.model.borrow_mut().draft.clear();
    ctx.model.borrow_mut().busy = false;
    ctx.model.borrow_mut().outgoing = false;
    if draft.is_empty() {
        return;
    }
    if buffer_text(&ctx.widgets.input.buffer()).trim().is_empty() {
        ctx.widgets.input.buffer().set_text(&draft);
    }
}

fn reload_transcript(ctx: &Ctx) {
    let id = ctx.model.borrow().conversation_id.clone();
    match id {
        Some(conversation_id) => ctx.command(Command::GetMessages { conversation_id }),
        None => ctx.widgets.transcript.buffer().set_text(""),
    }
}

fn append_outgoing(view: &gtk::TextView, text: &str) {
    let buffer = view.buffer();
    let mut end = buffer.end_iter();
    buffer.insert(&mut end, &format!("You\n{text}\n\n"));
    scroll_to_end(view);
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
