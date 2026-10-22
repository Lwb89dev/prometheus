use std::cell::{Cell, RefCell};
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::rc::Rc;
use std::time::Instant;

use gtk4::cairo::{Context, LinearGradient, RadialGradient};
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    AccessibleRole, DrawingArea, EventControllerFocus, EventControllerKey, EventControllerMotion,
    GestureClick, GestureDrag,
};

use crate::{DaemonVisualEvent, EnergyDirection, EntityEvent, EntityModel, SceneFrame};

#[derive(Default)]
pub struct EntityCallbacks {
    pub activate: Option<Box<dyn Fn()>>,
    pub context_menu: Option<Box<dyn Fn(f64, f64)>>,
    /// The host decides whether a compositor-supported move is possible.
    pub drag_update: Option<Box<dyn Fn(f64, f64)>>,
    pub drag_end: Option<Box<dyn Fn(f64, f64)>>,
    pub escape: Option<Box<dyn Fn()>>,
}

#[derive(Clone)]
pub struct EntityWidget {
    area: DrawingArea,
    model: Rc<RefCell<EntityModel>>,
    callbacks: Rc<RefCell<EntityCallbacks>>,
    epoch: Instant,
}

impl EntityWidget {
    pub fn new(size: i32) -> Self {
        let epoch = Instant::now();
        let model = Rc::new(RefCell::new(EntityModel::new(0.0)));
        let callbacks = Rc::new(RefCell::new(EntityCallbacks::default()));
        let area = DrawingArea::builder()
            .content_width(size.clamp(96, 512))
            .content_height(size.clamp(96, 512))
            .focusable(true)
            .accessible_role(AccessibleRole::Button)
            .tooltip_text("Prometeus — assistente desktop")
            .build();
        area.set_widget_name("prometeus-entity");
        area.update_property(&[
            gtk4::accessible::Property::Label("Prometeus"),
            gtk4::accessible::Property::Description("Assistente desktop pronto"),
        ]);

        install_renderer(&area, model.clone(), epoch);
        install_pointer(&area, model.clone(), callbacks.clone(), epoch);
        install_keyboard(&area, model.clone(), callbacks.clone(), epoch);
        install_frame_clock(&area, model.clone(), epoch);

        Self {
            area,
            model,
            callbacks,
            epoch,
        }
    }

    pub fn widget(&self) -> &DrawingArea {
        &self.area
    }

    pub fn model(&self) -> Rc<RefCell<EntityModel>> {
        self.model.clone()
    }

    pub fn set_callbacks(&self, callbacks: EntityCallbacks) {
        *self.callbacks.borrow_mut() = callbacks;
    }

    pub fn dispatch(&self, event: EntityEvent<'_>) {
        let now = self.epoch.elapsed().as_secs_f64();
        self.model.borrow_mut().dispatch(event, now);
        self.refresh_accessible_status(now);
        self.area.queue_draw();
    }

    pub fn apply_daemon_event(&self, event: DaemonVisualEvent<'_>) {
        let now = self.epoch.elapsed().as_secs_f64();
        self.model.borrow_mut().apply_daemon_event(event, now);
        self.refresh_accessible_status(now);
        self.area.queue_draw();
    }

    pub fn refresh_accessible_status(&self, now: f64) {
        let status = self.model.borrow_mut().sample(now);
        let description = status.accessible_description();
        self.area.set_tooltip_text(Some(&description));
        self.area
            .update_property(&[gtk4::accessible::Property::Description(&description)]);
    }
}

fn install_renderer(area: &DrawingArea, model: Rc<RefCell<EntityModel>>, epoch: Instant) {
    area.set_draw_func(move |_area, context, width, height| {
        let now = epoch.elapsed().as_secs_f64();
        let frame = model.borrow_mut().scene(now);
        draw_scene(context, width, height, &frame);
    });
}

fn install_frame_clock(area: &DrawingArea, model: Rc<RefCell<EntityModel>>, epoch: Instant) {
    area.add_tick_callback(move |area, frame_clock| {
        let now = epoch.elapsed().as_secs_f64();
        // Reading the frame time makes animation cadence display-driven; the
        // scene itself remains based on monotonic time and never fixed steps.
        let _display_frame_time = frame_clock.frame_time();
        let needs_frame = {
            let mut model = model.borrow_mut();
            model.dispatch(EntityEvent::SetVisible(area.is_mapped()), now);
            model.needs_frame()
        };
        if needs_frame {
            area.queue_draw();
        }
        glib::ControlFlow::Continue
    });
}

fn install_pointer(
    area: &DrawingArea,
    model: Rc<RefCell<EntityModel>>,
    callbacks: Rc<RefCell<EntityCallbacks>>,
    epoch: Instant,
) {
    let motion = EventControllerMotion::new();
    {
        let area = area.clone();
        let model = model.clone();
        motion.connect_enter(move |_motion, x, y| {
            let (x, y) = normalize(&area, x, y);
            model.borrow_mut().dispatch(
                EntityEvent::Hover { active: true, x, y },
                epoch.elapsed().as_secs_f64(),
            );
            area.queue_draw();
        });
    }
    {
        let area = area.clone();
        let model = model.clone();
        motion.connect_motion(move |_motion, x, y| {
            let (x, y) = normalize(&area, x, y);
            model.borrow_mut().dispatch(
                EntityEvent::Hover { active: true, x, y },
                epoch.elapsed().as_secs_f64(),
            );
        });
    }
    {
        let area = area.clone();
        let model = model.clone();
        motion.connect_leave(move |_motion| {
            model.borrow_mut().dispatch(
                EntityEvent::Hover {
                    active: false,
                    x: 0.0,
                    y: 0.0,
                },
                epoch.elapsed().as_secs_f64(),
            );
            area.queue_draw();
        });
    }
    area.add_controller(motion);

    let focus = EventControllerFocus::new();
    {
        let area = area.clone();
        let model = model.clone();
        focus.connect_enter(move |_focus| {
            model
                .borrow_mut()
                .dispatch(EntityEvent::Focus(true), epoch.elapsed().as_secs_f64());
            area.queue_draw();
        });
    }
    {
        let area = area.clone();
        let model = model.clone();
        focus.connect_leave(move |_focus| {
            model
                .borrow_mut()
                .dispatch(EntityEvent::Focus(false), epoch.elapsed().as_secs_f64());
            area.queue_draw();
        });
    }
    area.add_controller(focus);

    let was_dragged = Rc::new(Cell::new(false));
    let drag = GestureDrag::new();
    {
        let model = model.clone();
        let was_dragged = was_dragged.clone();
        drag.connect_drag_begin(move |_drag, _x, _y| {
            was_dragged.set(false);
            model
                .borrow_mut()
                .dispatch(EntityEvent::DragStarted, epoch.elapsed().as_secs_f64());
        });
    }
    {
        let area = area.clone();
        let model = model.clone();
        let callbacks = callbacks.clone();
        let was_dragged = was_dragged.clone();
        drag.connect_drag_update(move |_drag, dx, dy| {
            if dx.hypot(dy) >= 6.0 {
                was_dragged.set(true);
            }
            let width = f64::from(area.width().max(1));
            let height = f64::from(area.height().max(1));
            model.borrow_mut().dispatch(
                EntityEvent::Dragged {
                    x: (dx / width).clamp(-1.0, 1.0),
                    y: (dy / height).clamp(-1.0, 1.0),
                },
                epoch.elapsed().as_secs_f64(),
            );
            if was_dragged.get() {
                if let Some(callback) = &callbacks.borrow().drag_update {
                    callback(dx, dy);
                }
            }
        });
    }
    {
        let area = area.clone();
        let model = model.clone();
        let callbacks = callbacks.clone();
        let was_dragged = was_dragged.clone();
        drag.connect_drag_end(move |_drag, dx, dy| {
            model
                .borrow_mut()
                .dispatch(EntityEvent::DragEnded, epoch.elapsed().as_secs_f64());
            if was_dragged.get() {
                if let Some(callback) = &callbacks.borrow().drag_end {
                    callback(dx, dy);
                }
            }
            area.queue_draw();
        });
    }
    area.add_controller(drag);

    let click = GestureClick::new();
    click.set_button(0);
    {
        let area = area.clone();
        let model = model.clone();
        click.connect_pressed(move |gesture, _count, _x, _y| {
            if gesture.current_button() == 1 {
                model
                    .borrow_mut()
                    .dispatch(EntityEvent::Activated, epoch.elapsed().as_secs_f64());
                area.grab_focus();
                area.queue_draw();
            }
        });
    }
    {
        let callbacks = callbacks.clone();
        click.connect_released(
            move |gesture, _count, x, y| match gesture.current_button() {
                1 if !was_dragged.get() => {
                    if let Some(callback) = &callbacks.borrow().activate {
                        callback();
                    }
                }
                3 => {
                    if let Some(callback) = &callbacks.borrow().context_menu {
                        callback(x, y);
                    }
                }
                _ => {}
            },
        );
    }
    area.add_controller(click);
}

fn install_keyboard(
    area: &DrawingArea,
    model: Rc<RefCell<EntityModel>>,
    callbacks: Rc<RefCell<EntityCallbacks>>,
    epoch: Instant,
) {
    let key = EventControllerKey::new();
    {
        let area = area.clone();
        key.connect_key_pressed(move |_controller, key, _code, _state| match key {
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space => {
                model
                    .borrow_mut()
                    .dispatch(EntityEvent::Activated, epoch.elapsed().as_secs_f64());
                if let Some(callback) = &callbacks.borrow().activate {
                    callback();
                }
                area.queue_draw();
                glib::Propagation::Stop
            }
            gdk::Key::Escape => {
                if let Some(callback) = &callbacks.borrow().escape {
                    callback();
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
    }
    area.add_controller(key);
}

fn normalize(area: &DrawingArea, x: f64, y: f64) -> (f64, f64) {
    let width = f64::from(area.width().max(1));
    let height = f64::from(area.height().max(1));
    (
        (x / width * 2.0 - 1.0).clamp(-1.0, 1.0),
        (y / height * 2.0 - 1.0).clamp(-1.0, 1.0),
    )
}

fn draw_scene(context: &Context, width: i32, height: i32, frame: &SceneFrame) {
    let scale = f64::from(width.min(height)) / 200.0;
    let _ = context.save();
    context.translate(f64::from(width) * 0.5, f64::from(height) * 0.5);
    context.scale(scale, scale);
    context.rotate(frame.drag_tilt);
    context.set_line_cap(gtk4::cairo::LineCap::Round);
    context.set_line_join(gtk4::cairo::LineJoin::Round);

    draw_backplate(context, frame);
    draw_ring(context, frame);
    draw_surface_texture(context, frame);
    draw_circuits(context, frame);
    draw_chip(context, frame);
    draw_flame(context, frame);
    draw_particles(context, frame);
    if frame.focus_ring {
        for index in 0..6 {
            let start = -FRAC_PI_2 + f64::from(index) * TAU / 6.0 + 0.05;
            context.set_source_rgba(1.0, 0.72, 0.30, 0.70);
            context.set_line_width(if index % 2 == 0 { 1.8 } else { 1.0 });
            context.arc(0.0, 0.0, 95.0, start, start + 0.58);
            let _ = context.stroke();
        }
    }
    let _ = context.restore();
}

fn draw_backplate(context: &Context, frame: &SceneFrame) {
    context.set_source_rgba(0.0, 0.0, 0.0, 0.76);
    context.arc(0.0, 3.0, 94.0, 0.0, TAU);
    let _ = context.fill();

    let plate = RadialGradient::new(-8.0, -14.0, 3.0, 0.0, 4.0, 92.0);
    plate.add_color_stop_rgba(0.0, 0.13 + frame.error_tint * 0.07, 0.070, 0.035, 0.98);
    plate.add_color_stop_rgba(0.32, 0.055, 0.054, 0.057, 0.99);
    plate.add_color_stop_rgba(0.70, 0.024, 0.026, 0.031, 1.0);
    plate.add_color_stop_rgba(1.0, 0.004, 0.005, 0.007, 0.98);
    let _ = context.set_source(&plate);
    context.arc(0.0, 0.0, 90.0, 0.0, TAU);
    let _ = context.fill();

    let ember = RadialGradient::new(0.0, 8.0, 0.0, 0.0, 8.0, 68.0);
    ember.add_color_stop_rgba(0.0, 0.48, 0.12, 0.01, 0.11 + frame.chip_glow * 0.08);
    ember.add_color_stop_rgba(0.55, 0.20, 0.045, 0.0, 0.045);
    ember.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
    let _ = context.set_source(&ember);
    context.arc(0.0, 8.0, 68.0, 0.0, TAU);
    let _ = context.fill();
}

fn draw_ring(context: &Context, frame: &SceneFrame) {
    context.set_source_rgba(0.0, 0.0, 0.0, 0.86);
    context.set_line_width(8.5);
    context.arc(0.0, 0.0, 87.2, 0.0, TAU);
    let _ = context.stroke();

    let segments = [
        (-1.51, 0.73, 4.8, 0.84),
        (-0.69, 0.91, 3.4, 0.60),
        (0.34, 0.61, 5.2, 0.72),
        (1.07, 0.86, 3.6, 0.56),
        (2.07, 0.52, 4.5, 0.76),
        (2.72, 0.83, 3.0, 0.52),
        (3.70, 0.70, 5.0, 0.73),
        (4.55, 0.51, 3.4, 0.58),
    ];
    for (index, (start, length, width, alpha)) in segments.into_iter().enumerate() {
        let metal = LinearGradient::new(-90.0, -70.0, 84.0, 88.0);
        metal.add_color_stop_rgba(0.0, 0.08, 0.085, 0.095, alpha);
        metal.add_color_stop_rgba(0.22, 0.36, 0.33, 0.29, alpha);
        metal.add_color_stop_rgba(0.45, 0.11, 0.115, 0.125, alpha);
        metal.add_color_stop_rgba(0.73, 0.52, 0.31, 0.15, alpha * 0.78);
        metal.add_color_stop_rgba(1.0, 0.07, 0.075, 0.085, alpha);
        let _ = context.set_source(&metal);
        context.set_line_width(width);
        let radius = 87.0 + if index % 3 == 0 { 0.7 } else { 0.0 };
        context.arc(0.0, 0.0, radius, start, start + length);
        let _ = context.stroke();

        context.set_source_rgba(0.76, 0.48, 0.25, alpha * 0.30);
        context.set_line_width(0.65);
        context.arc(
            0.0,
            0.0,
            radius - width * 0.36,
            start + 0.03,
            start + length - 0.04,
        );
        let _ = context.stroke();
    }

    context.set_source_rgba(0.20, 0.19, 0.18, 0.72);
    context.set_line_width(0.75);
    context.arc(0.0, 0.0, 82.7, 0.0, TAU);
    let _ = context.stroke();

    for tick in 0..64 {
        if tick % 7 == 0 || tick % 11 == 0 {
            continue;
        }
        let angle = f64::from(tick) * TAU / 64.0;
        let outer = 84.0;
        let inner = outer - if tick % 8 == 0 { 3.8 } else { 1.9 };
        context.move_to(inner * angle.cos(), inner * angle.sin());
        context.line_to(outer * angle.cos(), outer * angle.sin());
        context.set_source_rgba(0.48, 0.39, 0.30, if tick % 8 == 0 { 0.46 } else { 0.22 });
        context.set_line_width(if tick % 8 == 0 { 1.0 } else { 0.55 });
        let _ = context.stroke();
    }

    for angle in [-1.12_f64, 0.58, 2.10, 3.52] {
        let x = angle.cos() * 87.0;
        let y = angle.sin() * 87.0;
        context.set_source_rgba(0.015, 0.016, 0.019, 0.95);
        context.arc(x, y, 2.4, 0.0, TAU);
        let _ = context.fill();
        context.set_source_rgba(0.56, 0.38, 0.22, 0.68);
        context.set_line_width(0.75);
        context.arc(x, y, 1.45, 0.0, TAU);
        let _ = context.stroke();
        context.move_to(x - 0.9, y + 0.4);
        context.line_to(x + 0.9, y - 0.4);
        let _ = context.stroke();
    }

    context.set_line_width(1.0 + frame.ring_glow * 1.6);
    context.set_source_rgba(1.0, 0.31, 0.0, 0.05 + frame.ring_glow * 0.42);
    let start = frame.ring_phase * TAU - FRAC_PI_2;
    context.arc(0.0, 0.0, 84.6, start, start + PI * 0.42);
    let _ = context.stroke();
}

fn draw_surface_texture(context: &Context, frame: &SceneFrame) {
    let _ = context.save();
    context.arc(0.0, 0.0, 82.0, 0.0, TAU);
    context.clip();

    for index in 0..110_u32 {
        let x = unit_hash(index * 17 + 3) * 160.0 - 80.0;
        let y = unit_hash(index * 29 + 11) * 160.0 - 80.0;
        if x * x + y * y > 6_600.0 {
            continue;
        }
        let warm = unit_hash(index * 41 + 7) > 0.78;
        if warm {
            context.set_source_rgba(0.72, 0.29, 0.06, 0.018 + frame.circuit_glow * 0.018);
        } else {
            context.set_source_rgba(0.68, 0.66, 0.61, 0.018);
        }
        let size = 0.25 + unit_hash(index * 13 + 5) * 0.55;
        context.rectangle(x, y, size, size * 0.45);
        let _ = context.fill();
    }

    for index in 0..18_u32 {
        let x = unit_hash(index * 53 + 1) * 116.0 - 58.0;
        let y = unit_hash(index * 31 + 9) * 125.0 - 60.0;
        let length = 3.0 + unit_hash(index * 73 + 4) * 12.0;
        let slope = unit_hash(index * 19 + 8) * 1.8 - 0.9;
        context.move_to(x, y);
        context.line_to(x + length, y + slope);
        context.set_source_rgba(0.52, 0.48, 0.42, 0.045);
        context.set_line_width(0.35);
        let _ = context.stroke();
    }
    let _ = context.restore();
}

fn draw_circuits(context: &Context, frame: &SceneFrame) {
    const PATHS: [[(f64, f64); 6]; 6] = [
        [
            (-23.0, 17.0),
            (-31.0, 17.0),
            (-38.0, 24.0),
            (-52.0, 24.0),
            (-61.0, 35.0),
            (-75.0, 35.0),
        ],
        [
            (-23.0, 28.0),
            (-34.0, 28.0),
            (-41.0, 42.0),
            (-54.0, 48.0),
            (-57.0, 61.0),
            (-68.0, 66.0),
        ],
        [
            (-12.0, 43.0),
            (-12.0, 52.0),
            (-21.0, 59.0),
            (-21.0, 70.0),
            (-29.0, 75.0),
            (-29.0, 81.0),
        ],
        [
            (23.0, 17.0),
            (32.0, 17.0),
            (39.0, 25.0),
            (51.0, 25.0),
            (62.0, 34.0),
            (75.0, 34.0),
        ],
        [
            (23.0, 29.0),
            (35.0, 29.0),
            (42.0, 41.0),
            (54.0, 47.0),
            (59.0, 59.0),
            (68.0, 65.0),
        ],
        [
            (12.0, 43.0),
            (12.0, 53.0),
            (22.0, 60.0),
            (22.0, 69.0),
            (30.0, 75.0),
            (30.0, 81.0),
        ],
    ];

    for (index, path) in PATHS.iter().enumerate() {
        stroke_polyline(context, path, (0.0, 0.0, 0.0, 0.86), 6.0);
        stroke_polyline(context, path, (0.23, 0.20, 0.17, 0.96), 3.4);
        stroke_polyline(context, path, (0.48, 0.31, 0.17, 0.72), 1.55);
        stroke_polyline(context, path, (0.77, 0.56, 0.34, 0.28), 0.55);
        if frame.circuit_glow > 0.01 {
            stroke_polyline(
                context,
                path,
                (
                    1.0,
                    0.31 + frame.error_tint * 0.03,
                    0.0,
                    frame.circuit_glow * 0.46,
                ),
                0.75,
            );
        }

        for (joint_index, joint) in path[1..path.len() - 1].iter().enumerate() {
            if joint_index % 2 == 0 {
                context.set_source_rgba(0.02, 0.018, 0.016, 0.95);
                context.arc(joint.0, joint.1, 2.35, 0.0, TAU);
                let _ = context.fill();
                context.set_source_rgba(0.48, 0.31, 0.18, 0.62);
                context.set_line_width(0.75);
                context.arc(joint.0, joint.1, 1.55, 0.0, TAU);
                let _ = context.stroke();
            }
        }

        let terminal = path[path.len() - 1];
        context.set_source_rgba(0.015, 0.014, 0.013, 0.96);
        hexagon(context, terminal.0, terminal.1, 4.6, PI / 6.0);
        let _ = context.fill();
        context.set_source_rgba(0.56, 0.35, 0.18, 0.74 + frame.circuit_glow * 0.20);
        context.set_line_width(1.1);
        hexagon(context, terminal.0, terminal.1, 3.5, PI / 6.0);
        let _ = context.stroke();
        context.set_source_rgba(0.90, 0.35, 0.04, 0.22 + frame.circuit_glow * 0.56);
        context.arc(terminal.0, terminal.1, 1.35, 0.0, TAU);
        let _ = context.fill();

        if frame.energy_direction != EnergyDirection::None {
            let phase = match frame.energy_direction {
                EnergyDirection::Inward => 1.0 - frame.circuit_phases[index],
                EnergyDirection::Outward => frame.circuit_phases[index],
                EnergyDirection::None => 0.0,
            };
            let (x, y) = point_on_polyline(path, phase);
            let pulse = RadialGradient::new(x, y, 0.0, x, y, 7.5);
            pulse.add_color_stop_rgba(0.0, 1.0, 0.80, 0.44, 0.92);
            pulse.add_color_stop_rgba(0.28, 1.0, 0.31, 0.0, 0.62);
            pulse.add_color_stop_rgba(1.0, 1.0, 0.20, 0.0, 0.0);
            let _ = context.set_source(&pulse);
            context.arc(x, y, 7.5, 0.0, TAU);
            let _ = context.fill();
            context.set_source_rgba(1.0, 0.84, 0.54, 0.92);
            hexagon(context, x, y, 1.75, frame.circuit_phases[index] * TAU);
            let _ = context.fill();
        }
    }

    for side in [-1.0_f64, 1.0] {
        let branch = [
            (side * 17.0, 10.0),
            (side * 28.0, 3.0),
            (side * 35.0, -8.0),
            (side * 48.0, -8.0),
        ];
        stroke_polyline(context, &branch, (0.16, 0.14, 0.13, 0.74), 2.4);
        stroke_polyline(context, &branch, (0.55, 0.33, 0.16, 0.30), 0.7);
    }
}

fn draw_chip(context: &Context, frame: &SceneFrame) {
    let glow = RadialGradient::new(0.0, 25.0, 1.0, 0.0, 25.0, 37.0);
    glow.add_color_stop_rgba(0.0, 1.0, 0.28, 0.0, frame.chip_glow * 0.32);
    glow.add_color_stop_rgba(0.48, 0.72, 0.14, 0.0, frame.chip_glow * 0.10);
    glow.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
    let _ = context.set_source(&glow);
    context.arc(0.0, 25.0, 37.0, 0.0, TAU);
    let _ = context.fill();

    for index in -3..=3 {
        let x = f64::from(index) * 5.2;
        let length = if index % 2 == 0 { 5.5 } else { 4.2 };
        context.set_source_rgba(0.01, 0.01, 0.012, 0.98);
        chamfered_rect(context, x - 1.8, 4.0 - length, 3.6, length + 2.0, 0.8);
        let _ = context.fill();
        chamfered_rect(context, x - 1.35, 5.0 - length, 2.7, length, 0.6);
        context.set_source_rgba(0.48, 0.31, 0.17, 0.82);
        let _ = context.fill();

        context.set_source_rgba(0.01, 0.01, 0.012, 0.98);
        chamfered_rect(context, x - 1.8, 45.0, 3.6, length + 2.0, 0.8);
        let _ = context.fill();
        chamfered_rect(context, x - 1.35, 45.0, 2.7, length, 0.6);
        context.set_source_rgba(0.48, 0.31, 0.17, 0.82);
        let _ = context.fill();
    }

    context.set_source_rgba(0.0, 0.0, 0.0, 0.84);
    chamfered_rect(context, -25.0, 6.0, 50.0, 42.0, 6.5);
    let _ = context.fill();
    let metal = LinearGradient::new(-24.0, 6.0, 24.0, 48.0);
    metal.add_color_stop_rgb(0.0, 0.38, 0.36, 0.33);
    metal.add_color_stop_rgb(0.16, 0.12, 0.125, 0.135);
    metal.add_color_stop_rgb(0.48, 0.055, 0.058, 0.065);
    metal.add_color_stop_rgb(0.76, 0.20, 0.15, 0.11);
    metal.add_color_stop_rgb(1.0, 0.46, 0.29, 0.15);
    chamfered_rect(context, -23.0, 5.0, 46.0, 40.0, 5.5);
    let _ = context.set_source(&metal);
    let _ = context.fill_preserve();
    context.set_source_rgba(0.62, 0.40, 0.23, 0.88);
    context.set_line_width(1.35);
    let _ = context.stroke();

    chamfered_rect(context, -19.5, 9.5, 39.0, 31.0, 3.3);
    context.set_source_rgba(0.018, 0.019, 0.023, 0.92);
    let _ = context.fill_preserve();
    context.set_source_rgba(0.43, 0.32, 0.24, 0.56);
    context.set_line_width(0.85);
    let _ = context.stroke();

    for row in 0..3 {
        for column in 0..4 {
            let x = -14.5 + f64::from(column) * 9.5;
            let y = 14.0 + f64::from(row) * 8.0;
            let active = (row + column) % 3 == 0;
            context.set_source_rgba(0.02, 0.019, 0.018, 0.96);
            chamfered_rect(context, x, y, 7.0, 5.4, 1.0);
            let _ = context.fill();
            context.set_source_rgba(
                0.72,
                0.24 + frame.error_tint * 0.04,
                0.02,
                if active {
                    0.12 + frame.chip_glow * 0.24
                } else {
                    0.055
                },
            );
            context.rectangle(x + 1.0, y + 1.0, 5.0, 0.65);
            let _ = context.fill();
        }
    }

    let core = RadialGradient::new(0.0, 25.0, 0.0, 0.0, 25.0, 10.5);
    core.add_color_stop_rgba(0.0, 1.0, 0.76, 0.37, 0.46 + frame.chip_glow * 0.50);
    core.add_color_stop_rgba(0.23, 1.0, 0.28, 0.0, 0.34 + frame.chip_glow * 0.44);
    core.add_color_stop_rgba(0.72, 0.22, 0.045, 0.0, 0.42);
    core.add_color_stop_rgba(1.0, 0.01, 0.01, 0.012, 0.0);
    let _ = context.set_source(&core);
    context.arc(0.0, 25.0, 10.5, 0.0, TAU);
    let _ = context.fill();
    context.set_source_rgba(0.92, 0.50, 0.19, 0.72);
    context.set_line_width(0.85);
    chamfered_rect(context, -5.5, 19.5, 11.0, 11.0, 2.2);
    let _ = context.stroke();

    for (x, y) in [(-18.0, 11.5), (18.0, 11.5), (-18.0, 38.5), (18.0, 38.5)] {
        context.set_source_rgba(0.015, 0.014, 0.013, 0.95);
        context.arc(x, y, 1.85, 0.0, TAU);
        let _ = context.fill();
        context.set_source_rgba(0.55, 0.41, 0.29, 0.58);
        context.set_line_width(0.55);
        context.move_to(x - 0.9, y + 0.25);
        context.line_to(x + 0.9, y - 0.25);
        let _ = context.stroke();
    }

    for index in 0..8_u32 {
        let x = -17.0 + unit_hash(index * 31 + 4) * 34.0;
        let y = 10.0 + unit_hash(index * 47 + 7) * 29.0;
        context.move_to(x, y);
        context.line_to(x + 2.0 + unit_hash(index * 11) * 5.0, y - 0.4);
        context.set_source_rgba(0.76, 0.67, 0.54, 0.075);
        context.set_line_width(0.38);
        let _ = context.stroke();
    }
}

fn draw_flame(context: &Context, frame: &SceneFrame) {
    let base_y = 3.0;
    let height = 52.0 * frame.flame_height;
    let half = 19.0 * frame.flame_width;
    let cx = frame.flame_x * 100.0;
    let bend = frame.flame_bend * 100.0;

    let aura = RadialGradient::new(
        cx,
        base_y - height * 0.38,
        1.0,
        cx,
        base_y - height * 0.34,
        39.0,
    );
    aura.add_color_stop_rgba(0.0, 1.0, 0.27, 0.0, frame.flame_intensity * 0.34);
    aura.add_color_stop_rgba(0.32, 0.78, 0.12, 0.0, frame.flame_intensity * 0.14);
    aura.add_color_stop_rgba(0.70, 0.30, 0.035, 0.0, frame.flame_intensity * 0.04);
    aura.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
    let _ = context.set_source(&aura);
    context.arc(cx, base_y - height * 0.34, 39.0, 0.0, TAU);
    let _ = context.fill();

    // A faint soot halo keeps the flame embedded in the dark scene instead of
    // looking like a clean logo laid on top of it.
    context.set_source_rgba(0.22, 0.16, 0.12, 0.12);
    context.set_line_width(2.2);
    context.move_to(cx - 7.0, base_y - height * 0.86);
    context.curve_to(
        cx - 14.0,
        base_y - height * 1.02,
        cx + 12.0,
        base_y - height * 1.07,
        cx + bend * 0.7,
        base_y - height * 1.20,
    );
    let _ = context.stroke();

    context.move_to(cx, base_y);
    context.curve_to(
        cx - half * 1.10,
        base_y - height * 0.16,
        cx - half * 0.92,
        base_y - height * 0.43,
        cx - half * 0.54,
        base_y - height * 0.55,
    );
    context.curve_to(
        cx - half * 0.18,
        base_y - height * 0.68,
        cx - half * 0.46 + bend * 0.35,
        base_y - height * 0.81,
        cx + bend,
        base_y - height,
    );
    context.curve_to(
        cx + half * 0.34 + bend,
        base_y - height * 0.80,
        cx + half * 0.12,
        base_y - height * 0.66,
        cx + half * 0.72,
        base_y - height * 0.52,
    );
    context.curve_to(
        cx + half * 1.22,
        base_y - height * 0.34,
        cx + half * 0.92,
        base_y - height * 0.10,
        cx,
        base_y,
    );
    context.close_path();
    let flame = LinearGradient::new(cx, base_y, cx + bend, base_y - height);
    flame.add_color_stop_rgba(0.0, 0.34, 0.055, 0.005, 0.96);
    flame.add_color_stop_rgba(0.20, 0.78, 0.15, 0.0, frame.flame_intensity.min(1.0));
    flame.add_color_stop_rgba(0.57, 1.0, 0.34, 0.0, frame.flame_intensity.min(1.0));
    flame.add_color_stop_rgba(
        0.82,
        1.0,
        0.57,
        0.10,
        (frame.flame_intensity * 0.95).min(1.0),
    );
    flame.add_color_stop_rgba(
        1.0,
        0.95,
        0.77,
        0.41,
        (frame.flame_intensity * 0.72).min(0.88),
    );
    let _ = context.set_source(&flame);
    let _ = context.fill();

    // Detached side tongue: deliberately irregular so the silhouette reads as
    // fire rather than a scaled vector droplet.
    context.move_to(cx - half * 0.42, base_y - height * 0.19);
    context.curve_to(
        cx - half * 0.95,
        base_y - height * 0.38,
        cx - half * 0.68,
        base_y - height * 0.56,
        cx - half * 0.48 + bend * 0.18,
        base_y - height * 0.69,
    );
    context.curve_to(
        cx - half * 0.15,
        base_y - height * 0.48,
        cx - half * 0.06,
        base_y - height * 0.32,
        cx - half * 0.42,
        base_y - height * 0.19,
    );
    context.close_path();
    context.set_source_rgba(0.94, 0.22, 0.0, frame.flame_intensity.min(0.78));
    let _ = context.fill();

    let inner_h = height * (0.44 + frame.inner_flame * 0.11);
    context.move_to(cx + 1.0, base_y - 1.0);
    context.curve_to(
        cx - half * 0.46,
        base_y - inner_h * 0.24,
        cx - half * 0.30,
        base_y - inner_h * 0.56,
        cx + bend * 0.24 - 1.0,
        base_y - inner_h,
    );
    context.curve_to(
        cx + half * 0.39,
        base_y - inner_h * 0.62,
        cx + half * 0.48,
        base_y - inner_h * 0.24,
        cx + 1.0,
        base_y - 1.0,
    );
    context.close_path();
    let inner = LinearGradient::new(cx, base_y, cx, base_y - inner_h);
    inner.add_color_stop_rgba(0.0, 1.0, 0.52, 0.06, 0.88);
    inner.add_color_stop_rgba(
        0.54,
        1.0,
        0.82,
        0.43,
        (0.40 + frame.inner_flame * 0.52).min(0.96),
    );
    inner.add_color_stop_rgba(1.0, 0.96, 0.87, 0.66, 0.74);
    let _ = context.set_source(&inner);
    let _ = context.fill();

    // A dark fissure adds depth and prevents the inner flame from reading as a
    // second flat shape.
    context.move_to(cx + 2.2, base_y - 2.0);
    context.curve_to(
        cx - 1.0,
        base_y - inner_h * 0.24,
        cx + 4.0,
        base_y - inner_h * 0.43,
        cx + bend * 0.12,
        base_y - inner_h * 0.62,
    );
    context.curve_to(
        cx + 7.0,
        base_y - inner_h * 0.38,
        cx + 6.0,
        base_y - inner_h * 0.17,
        cx + 2.2,
        base_y - 2.0,
    );
    context.close_path();
    context.set_source_rgba(0.20, 0.025, 0.0, 0.26);
    let _ = context.fill();
}

fn draw_particles(context: &Context, frame: &SceneFrame) {
    if frame.ember_particles <= 0.0 {
        return;
    }
    for index in 0..7_u32 {
        let phase = (frame.ember_phase + unit_hash(index * 37 + 3)).fract();
        let drift = (phase * TAU + f64::from(index)).sin() * (3.0 + phase * 8.0);
        let x = (unit_hash(index * 23 + 5) - 0.5) * 25.0 + drift;
        let y = -17.0 - phase * 69.0;
        let radius = 0.32 + unit_hash(index * 17 + 9) * 0.82;
        let alpha = (1.0 - phase).powf(1.4) * (0.25 + unit_hash(index * 41) * 0.55);
        let ember = RadialGradient::new(x, y, 0.0, x, y, radius * 3.0);
        ember.add_color_stop_rgba(
            0.0,
            1.0,
            0.82,
            0.43,
            (alpha * frame.ember_particles * 3.2).min(1.0),
        );
        ember.add_color_stop_rgba(
            0.42,
            1.0,
            0.25,
            0.0,
            (alpha * frame.ember_particles * 1.7).min(1.0),
        );
        ember.add_color_stop_rgba(1.0, 0.7, 0.1, 0.0, 0.0);
        let _ = context.set_source(&ember);
        context.arc(x, y, radius * 3.0, 0.0, TAU);
        let _ = context.fill();
    }
}

fn unit_hash(value: u32) -> f64 {
    let mixed = value.wrapping_mul(0x9E37_79B1).rotate_left(13) ^ 0xA5A5_A5A5;
    f64::from(mixed % 10_000) / 10_000.0
}

fn chamfered_rect(context: &Context, x: f64, y: f64, width: f64, height: f64, chamfer: f64) {
    let cut = chamfer.min(width / 2.0).min(height / 2.0);
    context.move_to(x + cut, y);
    context.line_to(x + width - cut, y);
    context.line_to(x + width, y + cut);
    context.line_to(x + width, y + height - cut);
    context.line_to(x + width - cut, y + height);
    context.line_to(x + cut, y + height);
    context.line_to(x, y + height - cut);
    context.line_to(x, y + cut);
    context.close_path();
}

fn hexagon(context: &Context, x: f64, y: f64, radius: f64, rotation: f64) {
    let mut angle = rotation;
    context.move_to(x + radius * angle.cos(), y + radius * angle.sin());
    for _ in 0..5 {
        angle += PI / 3.0;
        context.line_to(x + radius * angle.cos(), y + radius * angle.sin());
    }
    context.close_path();
}

fn stroke_polyline(
    context: &Context,
    points: &[(f64, f64)],
    rgba: (f64, f64, f64, f64),
    width: f64,
) {
    context.move_to(points[0].0, points[0].1);
    for point in &points[1..] {
        context.line_to(point.0, point.1);
    }
    context.set_source_rgba(rgba.0, rgba.1, rgba.2, rgba.3);
    context.set_line_width(width);
    context.set_line_join(gtk4::cairo::LineJoin::Round);
    let _ = context.stroke();
}

fn point_on_polyline(points: &[(f64, f64)], phase: f64) -> (f64, f64) {
    let total: f64 = points
        .windows(2)
        .map(|pair| (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1))
        .sum();
    let mut target = phase.clamp(0.0, 1.0) * total;
    for pair in points.windows(2) {
        let length = (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1);
        if target <= length {
            let amount = if length > 0.0 { target / length } else { 0.0 };
            return (
                pair[0].0 + (pair[1].0 - pair[0].0) * amount,
                pair[0].1 + (pair[1].1 - pair[0].1) * amount,
            );
        }
        target -= length;
    }
    points[points.len() - 1]
}

#[cfg(test)]
mod tests {
    use super::point_on_polyline;

    #[test]
    fn pulse_follows_all_segments() {
        let path = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        assert_eq!(point_on_polyline(&path, 0.25), (5.0, 0.0));
        assert_eq!(point_on_polyline(&path, 0.75), (10.0, 5.0));
    }
}
