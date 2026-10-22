use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box as GtkBox, Button, Label, Orientation, Scale};
use lda_visual::{Connection, EntityCallbacks, EntityEvent, EntityWidget};

fn main() -> gtk4::glib::ExitCode {
    let application = Application::builder()
        .application_id("org.linuxdesktopagent.Prometeus.VisualPreview")
        .build();
    application.connect_activate(build_preview);
    application.run()
}

fn build_preview(application: &Application) {
    let entity = EntityWidget::new(320);
    entity.dispatch(EntityEvent::Initialized);
    entity.set_callbacks(EntityCallbacks {
        activate: Some(Box::new(|| eprintln!("[preview] activate conversation"))),
        context_menu: Some(Box::new(|x, y| {
            eprintln!("[preview] context menu at {x:.0}, {y:.0}")
        })),
        drag_update: Some(Box::new(|dx, dy| {
            eprintln!("[preview] drag {dx:.0}, {dy:.0}")
        })),
        ..EntityCallbacks::default()
    });

    let controls = GtkBox::new(Orientation::Vertical, 6);
    controls.append(&Label::new(Some(
        "Development simulation — no backend events are emitted",
    )));
    controls.append(&event_button(
        "Idle",
        entity.clone(),
        EntityEvent::RequestCancelled,
    ));
    controls.append(&event_button(
        "Listening",
        entity.clone(),
        EntityEvent::ListeningStarted,
    ));
    controls.append(&event_button(
        "Transcribing",
        entity.clone(),
        EntityEvent::ListeningStopped,
    ));
    controls.append(&event_button(
        "Thinking",
        entity.clone(),
        EntityEvent::RequestStarted,
    ));
    controls.append(&event_button(
        "Responding",
        entity.clone(),
        EntityEvent::TextDelta,
    ));

    let tool = Button::with_label("Tool execution");
    {
        let entity = entity.clone();
        tool.connect_clicked(move |_| {
            entity.dispatch(EntityEvent::ToolStarted {
                id: "preview-tool",
                name: "get_system_info",
            });
        });
    }
    controls.append(&tool);
    controls.append(&event_button(
        "Success",
        entity.clone(),
        EntityEvent::RequestSucceeded,
    ));
    controls.append(&event_button(
        "Error",
        entity.clone(),
        EntityEvent::RequestFailed { recoverable: true },
    ));
    controls.append(&event_button(
        "Offline",
        entity.clone(),
        EntityEvent::SetConnection(Connection::Offline),
    ));
    controls.append(&event_button(
        "Online",
        entity.clone(),
        EntityEvent::SetConnection(Connection::Online),
    ));

    let amplitude = Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.01);
    amplitude.set_tooltip_text(Some("Simulated microphone amplitude"));
    {
        let entity = entity.clone();
        amplitude.connect_value_changed(move |scale| {
            entity.dispatch(EntityEvent::MicrophoneLevel(scale.value()));
        });
    }
    controls.append(&Label::new(Some("Simulated microphone amplitude")));
    controls.append(&amplitude);

    let row = GtkBox::new(Orientation::Horizontal, 18);
    row.set_margin_top(18);
    row.set_margin_bottom(18);
    row.set_margin_start(18);
    row.set_margin_end(18);
    row.append(entity.widget());
    row.append(&controls);

    let window = ApplicationWindow::builder()
        .application(application)
        .title("Prometeus Visual — Development Preview")
        .default_width(720)
        .default_height(420)
        .child(&row)
        .build();
    window.present();
}

fn event_button(label: &str, entity: EntityWidget, event: EntityEvent<'static>) -> Button {
    let button = Button::with_label(label);
    button.connect_clicked(move |_| entity.dispatch(event.clone()));
    button
}
