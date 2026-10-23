//! Push-to-talk. The microphone opens only while Talk is on.

use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use lda_common::{chat_refused, Transcript};
use lda_desktop::{play_wav, Capture};
use lda_visual::EntityEvent;

use crate::bus::Command;
use crate::window::{
    current_provider_id, dispatch_chat, provider_name, restore_draft, selected_kind, set_status,
    Ctx,
};

pub fn hook(ctx: &Rc<Ctx>) {
    let ctx_toggle = Rc::clone(ctx);
    ctx.widgets.talk.connect_toggled(move |button| {
        if ctx_toggle.model.borrow().talk_quiet {
            return;
        }
        if button.is_active() {
            start(&ctx_toggle);
            return;
        }
        stop(&ctx_toggle);
    });
    let ctx_close = Rc::clone(ctx);
    ctx.widgets.window.connect_close_request(move |_| {
        cancel(&ctx_close);
        glib::Propagation::Proceed
    });
}

pub fn route_typed(ctx: &Rc<Ctx>, text: &str) -> bool {
    let kind = selected_kind(ctx);
    if kind.speak && !kind.chat {
        speak(ctx, text);
        return true;
    }
    if kind.transcribe && !kind.chat {
        let name = provider_name(ctx);
        set_status(&ctx.widgets, &chat_refused(&name, kind));
        return true;
    }
    false
}

pub fn on_transcript(ctx: &Rc<Ctx>, transcript: Transcript) {
    ctx.model.borrow_mut().busy = false;
    let heard = transcript.text.trim();
    if heard.is_empty() {
        set_status(&ctx.widgets, "No words were recognized.");
        return;
    }
    if transcript.follow_up == "chat" {
        set_status(
            &ctx.widgets,
            &format!(
                "Heard with {}. Sending it to the conversation model.",
                transcript.model
            ),
        );
        dispatch_chat(ctx, transcript.text);
        return;
    }
    if transcript.follow_up == "speak" {
        set_status(&ctx.widgets, &format!("Heard with {}.", transcript.model));
        speak(ctx, &transcript.text);
        return;
    }
    ctx.widgets.input.buffer().set_text(&transcript.text);
    set_status(
        &ctx.widgets,
        &format!(
            "Transcribed with {}. This model does not continue a conversation.",
            transcript.model
        ),
    );
    ctx.widgets.input.grab_focus();
}

pub fn on_speech(ctx: &Rc<Ctx>, wav: Vec<u8>) {
    ctx.model.borrow_mut().busy = false;
    let draft = ctx.model.borrow().draft.clone();
    set_status(&ctx.widgets, "Playing…");
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(play_wav(&wav));
    });
    let ctx = Rc::clone(ctx);
    glib::spawn_future_local(async move {
        let played = rx.recv().await;
        finish_playback(&ctx, played, &draft);
    });
}

pub fn cancel(ctx: &Rc<Ctx>) {
    if ctx.capture.borrow().is_none() {
        return;
    }
    quiet_off(ctx);
    let capture = ctx.capture.borrow_mut().take();
    bump(ctx);
    if capture.is_some() {
        ctx.widgets.entity.dispatch(EntityEvent::ListeningStopped);
    }
}

fn start(ctx: &Rc<Ctx>) {
    if ctx.model.borrow().busy {
        quiet_off(ctx);
        set_status(&ctx.widgets, "A reply is still running.");
        return;
    }
    if ctx.capture.borrow().is_some() {
        return;
    }
    let capture = match Capture::start() {
        Ok(capture) => capture,
        Err(err) => {
            quiet_off(ctx);
            set_status(&ctx.widgets, &err.to_string());
            return;
        }
    };
    *ctx.capture.borrow_mut() = Some(capture);
    ctx.widgets.entity.dispatch(EntityEvent::ListeningStarted);
    set_status(
        &ctx.widgets,
        "Listening. Click Talk again to close the microphone.",
    );
    arm_limit(ctx);
}

fn stop(ctx: &Rc<Ctx>) {
    let Some(capture) = ctx.capture.borrow_mut().take() else {
        return;
    };
    bump(ctx);
    ctx.widgets.entity.dispatch(EntityEvent::ListeningStopped);
    set_status(&ctx.widgets, "Transcribing…");
    let (tx, rx) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(capture.stop());
    });
    let ctx = Rc::clone(ctx);
    glib::spawn_future_local(async move {
        let recorded = rx.recv().await;
        submit_wav(&ctx, recorded);
    });
}

fn submit_wav(
    ctx: &Rc<Ctx>,
    recorded: Result<lda_common::Result<Vec<u8>>, async_channel::RecvError>,
) {
    let wav = match recorded {
        Ok(Ok(wav)) => wav,
        Ok(Err(err)) => {
            set_status(&ctx.widgets, &err.to_string());
            return;
        }
        Err(_) => {
            set_status(&ctx.widgets, "The recording thread stopped.");
            return;
        }
    };
    let id = current_provider_id(ctx);
    ctx.model.borrow_mut().busy = true;
    let command = Command::Transcribe { id, wav };
    if ctx.cmd.try_send(command).is_err() {
        ctx.model.borrow_mut().busy = false;
        set_status(&ctx.widgets, "The background connection is closed.");
    }
}

fn speak(ctx: &Rc<Ctx>, text: &str) {
    if ctx.model.borrow().busy {
        set_status(&ctx.widgets, "A reply is still running.");
        return;
    }
    ctx.widgets.input.buffer().set_text("");
    ctx.model.borrow_mut().draft = text.to_string();
    ctx.model.borrow_mut().busy = true;
    set_status(&ctx.widgets, "Speaking…");
    let command = Command::Synthesize {
        id: current_provider_id(ctx),
        text: text.to_string(),
    };
    if ctx.cmd.try_send(command).is_err() {
        restore_draft(ctx);
        set_status(&ctx.widgets, "The background connection is closed.");
    }
}

fn finish_playback(
    ctx: &Ctx,
    played: Result<lda_common::Result<()>, async_channel::RecvError>,
    draft: &str,
) {
    match played {
        Ok(Ok(())) => {
            ctx.model.borrow_mut().draft.clear();
            set_status(&ctx.widgets, "Spoke.");
        }
        Ok(Err(err)) => {
            if ctx.widgets.input.buffer().char_count() == 0 {
                ctx.widgets.input.buffer().set_text(draft);
            }
            ctx.model.borrow_mut().draft.clear();
            set_status(&ctx.widgets, &err.to_string());
        }
        Err(_) => set_status(&ctx.widgets, "Playback stopped before it finished."),
    }
}

fn arm_limit(ctx: &Rc<Ctx>) {
    let generation = bump(ctx);
    let ctx = Rc::clone(ctx);
    glib::timeout_add_local(Duration::from_secs(30), move || {
        if ctx.model.borrow().talk_generation != generation {
            return glib::ControlFlow::Break;
        }
        set_status(&ctx.widgets, "Closed the microphone after 30 seconds.");
        quiet_off(&ctx);
        stop(&ctx);
        glib::ControlFlow::Break
    });
}

fn quiet_off(ctx: &Ctx) {
    ctx.model.borrow_mut().talk_quiet = true;
    ctx.widgets.talk.set_active(false);
    ctx.model.borrow_mut().talk_quiet = false;
}

fn bump(ctx: &Ctx) -> u64 {
    let mut model = ctx.model.borrow_mut();
    model.talk_generation = model.talk_generation.wrapping_add(1);
    model.talk_generation
}
