//! Saved notes. The window lists and deletes them. It does not invent new ones.

use std::rc::Rc;

use gtk::{glib, prelude::*};
use lda_common::MemoryNote;

use crate::bus::Command;
use crate::window::{set_status, Ctx};

pub fn open(ctx: &Rc<Ctx>) {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Browse);
    list.update_property(&[gtk::accessible::Property::Label("Saved notes")]);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .min_content_height(240)
        .build();
    let delete = gtk::Button::with_label("Delete note");
    let hint = gtk::Label::builder()
        .label("Notes appear only when you save them. New notes stay off until persistent memory is enabled. Delete still removes a note that is already stored.")
        .wrap(true)
        .xalign(0.0)
        .build();
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.append(&hint);
    root.append(&scroll);
    root.append(&delete);
    let dialog = gtk::Window::builder()
        .transient_for(&ctx.widgets.window)
        .modal(true)
        .title("Notes")
        .default_width(440)
        .default_height(380)
        .child(&root)
        .build();
    ctx.model.borrow_mut().notes_list = Some(list);
    let ctx_close = Rc::clone(ctx);
    dialog.connect_close_request(move |_| {
        ctx_close.model.borrow_mut().notes_list = None;
        glib::Propagation::Proceed
    });
    let ctx_delete = Rc::clone(ctx);
    delete.connect_clicked(move |_| delete_selected(&ctx_delete));
    ctx.command(Command::ListMemories);
    dialog.present();
}

pub fn show(ctx: &Ctx, notes: &[MemoryNote]) {
    let Some(list) = ctx.model.borrow().notes_list.clone() else {
        return;
    };
    clear(&list);
    if notes.is_empty() {
        let empty = gtk::Label::builder()
            .label("No saved notes.")
            .xalign(0.0)
            .build();
        list.append(&empty);
        return;
    }
    for note in notes {
        let row = gtk::Label::builder()
            .label(&note.body)
            .wrap(true)
            .xalign(0.0)
            .build();
        row.set_widget_name(&note.id);
        list.append(&row);
    }
}

fn delete_selected(ctx: &Rc<Ctx>) {
    let Some(id) = selected_id(ctx) else {
        set_status(&ctx.widgets, "Select a note first.");
        return;
    };
    confirm(
        ctx,
        "Delete this note?",
        "The note is removed from this computer.",
        move |ctx| ctx.command(Command::ForgetMemory { id }),
    );
}

fn selected_id(ctx: &Ctx) -> Option<String> {
    let list = ctx.model.borrow().notes_list.clone()?;
    let row = list.selected_row()?;
    let child = row.child()?;
    let id = child.widget_name().to_string();
    if is_memory_id(&id) {
        return Some(id);
    }
    None
}

fn is_memory_id(id: &str) -> bool {
    id.len() == 32 && id.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn clear(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
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
