use crate::model::{
    Activity, Connection, FeedbackKind, MotionMode, PermissionState, ToolCategory, VisualStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnergyDirection {
    None,
    Inward,
    Outward,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneFrame {
    pub flame_height: f64,
    pub flame_width: f64,
    pub flame_x: f64,
    pub flame_bend: f64,
    pub flame_intensity: f64,
    pub inner_flame: f64,
    pub chip_glow: f64,
    pub circuit_glow: f64,
    pub ring_glow: f64,
    pub ring_phase: f64,
    pub ember_phase: f64,
    pub energy_direction: EnergyDirection,
    pub circuit_phases: [f64; 6],
    pub error_tint: f64,
    pub focus_ring: bool,
    pub drag_tilt: f64,
    pub ember_particles: f64,
}

#[derive(Clone, Copy)]
struct Profile {
    flame_height: f64,
    flame_width: f64,
    flame_intensity: f64,
    inner_flame: f64,
    chip_glow: f64,
    circuit_glow: f64,
    ring_glow: f64,
}

impl SceneFrame {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_status(
        status: &VisualStatus,
        previous: Activity,
        motion: MotionMode,
        intensity: f64,
        pointer_x: f64,
        _pointer_y: f64,
        category: Option<ToolCategory>,
        now: f64,
    ) -> Self {
        let static_motion = motion == MotionMode::Static;
        let reduced = motion == MotionMode::Reduced;
        let time = if static_motion { 0.0 } else { now.max(0.0) };
        let mut current = profile(status.activity);
        let prior = profile(previous);
        current = blend_profile(prior, current, status.transition);

        let offline = status.connection == Connection::Offline;
        if offline {
            current = Profile {
                flame_height: 0.28,
                flame_width: 0.34,
                flame_intensity: 0.26,
                inner_flame: 0.12,
                chip_glow: 0.08,
                circuit_glow: 0.02,
                ring_glow: 0.16,
            };
        }

        let organic =
            wave(time, 1.73, 0.11) + wave(time, 2.91, 1.7) * 0.45 + wave(time, 5.13, 4.0) * 0.2;
        let slow = wave(time, 0.73, 0.4);
        let hover = if status.hovered { 1.0 } else { 0.0 };
        let mic = if status.activity == Activity::Listening {
            status.microphone_level
        } else {
            0.0
        };
        let responding = if status.activity == Activity::Responding {
            (wave(time, 1.35, 0.0) + 1.0) * 0.5
        } else {
            0.0
        };
        let thinking = status.activity == Activity::Thinking;
        let tool_active = status.active_tools > 0;
        let animated_amount = if reduced { 0.22 } else { 1.0 } * intensity;

        let direction = if offline || static_motion || reduced {
            EnergyDirection::None
        } else if tool_active {
            EnergyDirection::Outward
        } else if thinking || status.activity == Activity::Transcribing {
            EnergyDirection::Inward
        } else if status.activity == Activity::Responding {
            EnergyDirection::Outward
        } else {
            EnergyDirection::None
        };

        let category_speed = match category {
            Some(ToolCategory::SystemDiagnostics) => 1.35,
            Some(ToolCategory::WebResearch) => 0.78,
            Some(ToolCategory::FileOperation) => 0.95,
            Some(ToolCategory::SystemModification) => 0.55,
            _ => 1.0,
        };
        let phase_time = time * category_speed;
        let circuit_phases = std::array::from_fn(|index| {
            let stagger = [0.0, 0.17, 0.41, 0.09, 0.31, 0.52][index];
            (phase_time * 0.72 + stagger).fract()
        });

        let error_tint = match status.feedback {
            Some(FeedbackKind::Error { .. }) => 1.0,
            _ if status.permission == PermissionState::Required => 0.55,
            _ => 0.0,
        };
        let success_boost =
            matches!(status.feedback, Some(FeedbackKind::Success)) as u8 as f64 * 0.24;
        let activation_boost =
            matches!(status.feedback, Some(FeedbackKind::Activation)) as u8 as f64 * 0.12;
        let procedural = if static_motion || offline {
            0.0
        } else {
            organic * animated_amount
        };

        Self {
            flame_height: (current.flame_height + procedural * 0.035 + mic * 0.12).clamp(0.2, 1.18),
            flame_width: (current.flame_width - thinking as u8 as f64 * 0.04 + procedural * 0.018)
                .clamp(0.25, 0.9),
            flame_x: if static_motion {
                0.0
            } else {
                procedural * 0.014 + pointer_x * hover * 0.025
            },
            flame_bend: if static_motion {
                0.0
            } else {
                wave(time, 1.11, 2.0) * 0.055 * animated_amount
            },
            flame_intensity: (current.flame_intensity
                + hover * 0.13
                + mic * 0.25
                + responding * 0.06
                + success_boost
                + activation_boost
                + slow * 0.025 * animated_amount)
                .clamp(0.05, 1.25),
            inner_flame: (current.inner_flame + mic * 0.18 + procedural * 0.025).clamp(0.0, 1.2),
            chip_glow: (current.chip_glow + hover * 0.14 + responding * 0.08 + success_boost)
                .clamp(0.0, 1.2),
            circuit_glow: (current.circuit_glow + hover * 0.13 + success_boost).clamp(0.0, 1.0),
            ring_glow: (current.ring_glow + mic * 0.12 + success_boost).clamp(0.0, 1.0),
            ring_phase: if static_motion || reduced {
                0.0
            } else {
                (time * 0.055).fract()
            },
            ember_phase: if static_motion || reduced {
                0.0
            } else {
                (time * 0.19).fract()
            },
            energy_direction: direction,
            circuit_phases,
            error_tint,
            focus_ring: status.focused,
            drag_tilt: if status.dragging && !static_motion {
                pointer_x * 0.055 * if reduced { 0.2 } else { 1.0 }
            } else {
                0.0
            },
            ember_particles: if reduced || static_motion || offline {
                0.0
            } else {
                0.16 * intensity
            },
        }
    }
}

fn profile(activity: Activity) -> Profile {
    match activity {
        Activity::Initializing => Profile {
            flame_height: 0.48,
            flame_width: 0.45,
            flame_intensity: 0.42,
            inner_flame: 0.28,
            chip_glow: 0.32,
            circuit_glow: 0.18,
            ring_glow: 0.28,
        },
        Activity::Idle => Profile {
            flame_height: 0.72,
            flame_width: 0.58,
            flame_intensity: 0.72,
            inner_flame: 0.55,
            chip_glow: 0.28,
            circuit_glow: 0.10,
            ring_glow: 0.25,
        },
        Activity::Listening => Profile {
            flame_height: 0.88,
            flame_width: 0.62,
            flame_intensity: 0.94,
            inner_flame: 0.78,
            chip_glow: 0.54,
            circuit_glow: 0.40,
            ring_glow: 0.52,
        },
        Activity::Transcribing => Profile {
            flame_height: 0.76,
            flame_width: 0.48,
            flame_intensity: 0.82,
            inner_flame: 0.72,
            chip_glow: 0.70,
            circuit_glow: 0.48,
            ring_glow: 0.42,
        },
        Activity::Thinking => Profile {
            flame_height: 0.86,
            flame_width: 0.43,
            flame_intensity: 0.91,
            inner_flame: 0.88,
            chip_glow: 0.84,
            circuit_glow: 0.66,
            ring_glow: 0.44,
        },
        Activity::Responding => Profile {
            flame_height: 0.79,
            flame_width: 0.54,
            flame_intensity: 0.84,
            inner_flame: 0.68,
            chip_glow: 0.60,
            circuit_glow: 0.44,
            ring_glow: 0.36,
        },
        Activity::Disabled => Profile {
            flame_height: 0.22,
            flame_width: 0.30,
            flame_intensity: 0.12,
            inner_flame: 0.04,
            chip_glow: 0.04,
            circuit_glow: 0.02,
            ring_glow: 0.12,
        },
    }
}

fn blend_profile(from: Profile, to: Profile, amount: f64) -> Profile {
    let blend = |a: f64, b: f64| a + (b - a) * amount;
    Profile {
        flame_height: blend(from.flame_height, to.flame_height),
        flame_width: blend(from.flame_width, to.flame_width),
        flame_intensity: blend(from.flame_intensity, to.flame_intensity),
        inner_flame: blend(from.inner_flame, to.inner_flame),
        chip_glow: blend(from.chip_glow, to.chip_glow),
        circuit_glow: blend(from.circuit_glow, to.circuit_glow),
        ring_glow: blend(from.ring_glow, to.ring_glow),
    }
}

fn wave(time: f64, frequency: f64, phase: f64) -> f64 {
    (time * frequency * std::f64::consts::TAU + phase).sin()
}

#[cfg(test)]
mod tests {
    use crate::{EntityEvent, EntityModel, MotionMode};

    use super::*;

    #[test]
    fn thinking_converges_and_tools_propagate_outward() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(EntityEvent::Initialized, 0.0);
        model.dispatch(EntityEvent::RequestStarted, 1.0);
        let thinking = model.scene(2.0);
        assert_eq!(thinking.energy_direction, EnergyDirection::Inward);
        model.dispatch(
            EntityEvent::ToolStarted {
                id: "a",
                name: "get_system_info",
            },
            2.1,
        );
        assert_eq!(model.scene(2.5).energy_direction, EnergyDirection::Outward);
    }

    #[test]
    fn static_mode_is_time_invariant_after_transition() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(EntityEvent::Initialized, 0.0);
        model.dispatch(EntityEvent::SetMotion(MotionMode::Static), 0.0);
        assert_eq!(model.scene(10.0), model.scene(100.0));
    }

    #[test]
    fn reduced_motion_uses_static_state_cues() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(EntityEvent::SetMotion(MotionMode::Reduced), 0.0);
        model.dispatch(EntityEvent::RequestStarted, 0.1);
        let frame = model.scene(1.0);
        assert_eq!(frame.energy_direction, EnergyDirection::None);
        assert_eq!(frame.ring_phase, 0.0);
        assert_eq!(frame.ember_particles, 0.0);
        assert!(frame.chip_glow > 0.5);
    }
}
