use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

const FEEDBACK_SECONDS: f64 = 1.1;
const CLICK_SECONDS: f64 = 0.22;
const TRANSITION_SECONDS: f64 = 0.28;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Initializing,
    #[default]
    Idle,
    Listening,
    Transcribing,
    Thinking,
    Responding,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    #[default]
    Online,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MotionMode {
    #[default]
    Full,
    Reduced,
    Static,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    SystemDiagnostics,
    WebResearch,
    FileOperation,
    SystemModification,
    Other,
}

impl ToolCategory {
    pub fn from_name(name: &str) -> Self {
        let name = name.to_ascii_lowercase();
        if name.contains("web") || name.contains("search") || name.contains("http") {
            Self::WebResearch
        } else if name.contains("file") || name.contains("read") || name.contains("write") {
            Self::FileOperation
        } else if name.contains("install") || name.contains("remove") || name.contains("modify") {
            Self::SystemModification
        } else if name.contains("system")
            || name.contains("cpu")
            || name.contains("memory")
            || name.contains("network")
            || name.contains("kernel")
            || name.contains("hardware")
        {
            Self::SystemDiagnostics
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PermissionState {
    #[default]
    None,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackKind {
    Success,
    Error { recoverable: bool },
    Activation,
}

#[derive(Debug, Clone)]
struct Feedback {
    kind: FeedbackKind,
    until: f64,
}

#[derive(Debug, Clone)]
struct ActiveTool {
    name: String,
    category: ToolCategory,
}

#[derive(Debug, Clone)]
pub enum EntityEvent<'a> {
    Initialized,
    SetVisible(bool),
    SetMotion(MotionMode),
    SetIntensity(f64),
    SetConnection(Connection),
    SetEnabled(bool),
    Hover { active: bool, x: f64, y: f64 },
    Focus(bool),
    DragStarted,
    Dragged { x: f64, y: f64 },
    DragEnded,
    Activated,
    ListeningStarted,
    MicrophoneLevel(f64),
    ListeningStopped,
    TranscriptionSubmitted,
    RequestStarted,
    TextDelta,
    ToolStarted { id: &'a str, name: &'a str },
    ToolFinished { id: &'a str, ok: bool },
    PermissionRequired,
    PermissionResolved,
    RequestSucceeded,
    RequestFailed { recoverable: bool },
    RequestCancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VisualStatus {
    pub activity: Activity,
    pub connection: Connection,
    pub hovered: bool,
    pub focused: bool,
    pub dragging: bool,
    pub visible: bool,
    pub permission: PermissionState,
    pub active_tools: usize,
    pub active_tool_name: Option<String>,
    pub feedback: Option<FeedbackKind>,
    pub microphone_level: f64,
    pub transition: f64,
}

impl VisualStatus {
    pub fn accessible_description(&self) -> String {
        let base = if self.connection == Connection::Offline {
            "Prometeus non connesso".to_string()
        } else if self.permission == PermissionState::Required {
            "Prometeus richiede un'autorizzazione".to_string()
        } else if let Some(name) = &self.active_tool_name {
            format!("Prometeus sta eseguendo {name}")
        } else {
            match self.activity {
                Activity::Initializing => "Prometeus si sta avviando",
                Activity::Idle => "Prometeus è pronto",
                Activity::Listening => "Prometeus sta ascoltando",
                Activity::Transcribing => "Prometeus sta trascrivendo",
                Activity::Thinking => "Prometeus sta elaborando",
                Activity::Responding => "Prometeus sta rispondendo",
                Activity::Disabled => "Prometeus è disattivato",
            }
            .to_string()
        };
        match self.feedback {
            Some(FeedbackKind::Error { .. }) => format!("{base}. Ultima operazione non riuscita"),
            Some(FeedbackKind::Success) => format!("{base}. Operazione completata"),
            _ => base,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EntityModel {
    activity: Activity,
    previous_activity: Activity,
    connection: Connection,
    motion: MotionMode,
    permission: PermissionState,
    visible: bool,
    hovered: bool,
    focused: bool,
    dragging: bool,
    pointer_x: f64,
    pointer_y: f64,
    microphone_target: f64,
    microphone_level: f64,
    intensity: f64,
    tools: BTreeMap<String, ActiveTool>,
    feedback: Option<Feedback>,
    changed_at: f64,
    sampled_at: f64,
}

impl Default for EntityModel {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl EntityModel {
    pub fn new(now: f64) -> Self {
        Self {
            activity: Activity::Initializing,
            previous_activity: Activity::Initializing,
            connection: Connection::Online,
            motion: MotionMode::Full,
            permission: PermissionState::None,
            visible: true,
            hovered: false,
            focused: false,
            dragging: false,
            pointer_x: 0.0,
            pointer_y: 0.0,
            microphone_target: 0.0,
            microphone_level: 0.0,
            intensity: 1.0,
            tools: BTreeMap::new(),
            feedback: None,
            changed_at: now,
            sampled_at: now,
        }
    }

    pub fn dispatch(&mut self, event: EntityEvent<'_>, now: f64) {
        self.expire_feedback(now);
        match event {
            EntityEvent::Initialized => self.set_activity(Activity::Idle, now),
            EntityEvent::SetVisible(value) => self.visible = value,
            EntityEvent::SetMotion(value) => self.motion = value,
            EntityEvent::SetIntensity(value) => self.intensity = value.clamp(0.0, 1.0),
            EntityEvent::SetConnection(value) => {
                self.connection = value;
                self.changed_at = now;
                if value == Connection::Offline {
                    self.tools.clear();
                    self.permission = PermissionState::None;
                    self.microphone_target = 0.0;
                }
            }
            EntityEvent::SetEnabled(enabled) => self.set_activity(
                if enabled {
                    Activity::Idle
                } else {
                    Activity::Disabled
                },
                now,
            ),
            EntityEvent::Hover { active, x, y } => {
                self.hovered = active;
                self.pointer_x = normalized_pointer(x);
                self.pointer_y = normalized_pointer(y);
            }
            EntityEvent::Focus(value) => self.focused = value,
            EntityEvent::DragStarted => self.dragging = true,
            EntityEvent::Dragged { x, y } => {
                self.dragging = true;
                self.pointer_x = normalized_pointer(x);
                self.pointer_y = normalized_pointer(y);
            }
            EntityEvent::DragEnded => self.dragging = false,
            EntityEvent::Activated => {
                self.feedback = Some(Feedback {
                    kind: FeedbackKind::Activation,
                    until: now + CLICK_SECONDS,
                });
            }
            EntityEvent::ListeningStarted if self.activity != Activity::Disabled => {
                self.set_activity(Activity::Listening, now)
            }
            EntityEvent::MicrophoneLevel(value) => {
                // Noise gate and clamp happen before temporal smoothing in `sample`.
                self.microphone_target = if value.is_finite() && value > 0.035 {
                    ((value - 0.035) / 0.965).clamp(0.0, 1.0)
                } else {
                    0.0
                };
            }
            EntityEvent::ListeningStopped if self.activity == Activity::Listening => {
                self.microphone_target = 0.0;
                self.set_activity(Activity::Transcribing, now);
            }
            EntityEvent::TranscriptionSubmitted | EntityEvent::RequestStarted
                if self.activity != Activity::Disabled =>
            {
                self.set_activity(Activity::Thinking, now);
            }
            EntityEvent::TextDelta if self.activity != Activity::Disabled => {
                self.set_activity(Activity::Responding, now)
            }
            EntityEvent::ToolStarted { id, name } if self.activity != Activity::Disabled => {
                self.tools.insert(
                    id.to_string(),
                    ActiveTool {
                        name: name.to_string(),
                        category: ToolCategory::from_name(name),
                    },
                );
                self.changed_at = now;
            }
            EntityEvent::ToolFinished { id, ok } => {
                self.tools.remove(id);
                if !ok {
                    self.feedback = Some(Feedback {
                        kind: FeedbackKind::Error { recoverable: true },
                        until: now + FEEDBACK_SECONDS,
                    });
                }
                if self.tools.is_empty() && self.activity != Activity::Disabled {
                    self.set_activity(Activity::Thinking, now);
                }
            }
            EntityEvent::PermissionRequired => {
                self.permission = PermissionState::Required;
                self.changed_at = now;
            }
            EntityEvent::PermissionResolved => {
                self.permission = PermissionState::None;
                self.changed_at = now;
            }
            EntityEvent::RequestSucceeded => {
                self.tools.clear();
                self.permission = PermissionState::None;
                self.set_activity(Activity::Idle, now);
                self.feedback = Some(Feedback {
                    kind: FeedbackKind::Success,
                    until: now + FEEDBACK_SECONDS,
                });
            }
            EntityEvent::RequestFailed { recoverable } => {
                self.tools.clear();
                self.permission = PermissionState::None;
                self.set_activity(Activity::Idle, now);
                self.feedback = Some(Feedback {
                    kind: FeedbackKind::Error { recoverable },
                    until: now + FEEDBACK_SECONDS,
                });
            }
            EntityEvent::RequestCancelled => {
                self.tools.clear();
                self.permission = PermissionState::None;
                self.set_activity(Activity::Idle, now);
                self.feedback = None;
            }
            _ => {}
        }
    }

    pub fn sample(&mut self, now: f64) -> VisualStatus {
        self.expire_feedback(now);
        let dt = (now - self.sampled_at).clamp(0.0, 0.25);
        self.sampled_at = now.max(self.sampled_at);
        let rate = if self.microphone_target > self.microphone_level {
            18.0 // fast attack
        } else {
            5.0 // slower release
        };
        let alpha = 1.0 - (-rate * dt).exp();
        self.microphone_level += (self.microphone_target - self.microphone_level) * alpha;

        let transition = if self.motion == MotionMode::Static {
            1.0
        } else {
            smoothstep(((now - self.changed_at) / TRANSITION_SECONDS).clamp(0.0, 1.0))
        };
        VisualStatus {
            activity: self.activity,
            connection: self.connection,
            hovered: self.hovered,
            focused: self.focused,
            dragging: self.dragging,
            visible: self.visible,
            permission: self.permission,
            active_tools: self.tools.len(),
            active_tool_name: self.tools.values().next().map(|tool| tool.name.clone()),
            feedback: self.feedback.as_ref().map(|value| value.kind),
            microphone_level: self.microphone_level,
            transition,
        }
    }

    pub fn scene(&mut self, now: f64) -> crate::SceneFrame {
        let status = self.sample(now);
        let category = self.tools.values().next().map(|tool| tool.category);
        crate::scene::SceneFrame::from_status(
            &status,
            self.previous_activity,
            self.motion,
            self.intensity,
            self.pointer_x,
            self.pointer_y,
            category,
            now,
        )
    }

    pub fn needs_frame(&self) -> bool {
        self.visible
            && self.motion != MotionMode::Static
            && self.activity != Activity::Disabled
            && self.connection != Connection::Offline
    }

    fn set_activity(&mut self, activity: Activity, now: f64) {
        if self.activity != activity {
            self.previous_activity = self.activity;
            self.activity = activity;
            self.changed_at = now;
        }
    }

    fn expire_feedback(&mut self, now: f64) {
        if self
            .feedback
            .as_ref()
            .is_some_and(|feedback| now >= feedback.until)
        {
            self.feedback = None;
        }
    }
}

fn normalized_pointer(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn smoothstep(value: f64) -> f64 {
    value * value * (3.0 - 2.0 * value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_is_orthogonal_to_tool_activity() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(EntityEvent::Initialized, 0.0);
        model.dispatch(EntityEvent::RequestStarted, 1.0);
        model.dispatch(
            EntityEvent::ToolStarted {
                id: "a",
                name: "get_cpu_info",
            },
            2.0,
        );
        model.dispatch(
            EntityEvent::Hover {
                active: true,
                x: 0.4,
                y: -0.3,
            },
            2.1,
        );
        let status = model.sample(2.2);
        assert!(status.hovered);
        assert_eq!(status.active_tools, 1);
        assert_eq!(status.activity, Activity::Thinking);
    }

    #[test]
    fn realistic_tool_sequence_returns_to_idle() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(EntityEvent::Initialized, 0.1);
        model.dispatch(EntityEvent::RequestStarted, 1.0);
        model.dispatch(
            EntityEvent::ToolStarted {
                id: "a",
                name: "get_system_info",
            },
            2.0,
        );
        model.dispatch(EntityEvent::ToolFinished { id: "a", ok: true }, 3.0);
        model.dispatch(EntityEvent::TextDelta, 4.0);
        model.dispatch(EntityEvent::RequestSucceeded, 5.0);
        let status = model.sample(5.1);
        assert_eq!(status.activity, Activity::Idle);
        assert_eq!(status.feedback, Some(FeedbackKind::Success));
    }

    #[test]
    fn cancellation_never_looks_successful() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(EntityEvent::RequestStarted, 1.0);
        model.dispatch(EntityEvent::TextDelta, 2.0);
        model.dispatch(EntityEvent::RequestCancelled, 3.0);
        let status = model.sample(3.1);
        assert_eq!(status.activity, Activity::Idle);
        assert_eq!(status.feedback, None);
    }

    #[test]
    fn concurrent_tools_do_not_end_early() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(EntityEvent::RequestStarted, 0.1);
        model.dispatch(
            EntityEvent::ToolStarted {
                id: "a",
                name: "get_cpu_info",
            },
            0.2,
        );
        model.dispatch(
            EntityEvent::ToolStarted {
                id: "b",
                name: "get_memory_info",
            },
            0.3,
        );
        model.dispatch(EntityEvent::ToolFinished { id: "a", ok: true }, 0.4);
        assert_eq!(model.sample(0.5).active_tools, 1);
    }

    #[test]
    fn microphone_has_gate_attack_and_release() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(EntityEvent::ListeningStarted, 0.0);
        model.dispatch(EntityEvent::MicrophoneLevel(0.02), 0.0);
        assert_eq!(model.sample(0.1).microphone_level, 0.0);
        model.dispatch(EntityEvent::MicrophoneLevel(0.8), 0.1);
        let high = model.sample(0.2).microphone_level;
        assert!(high > 0.5);
        model.dispatch(EntityEvent::MicrophoneLevel(0.0), 0.2);
        assert!(model.sample(0.3).microphone_level > 0.0);
    }

    #[test]
    fn hidden_or_static_entity_does_not_request_frames() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(EntityEvent::Initialized, 0.0);
        model.dispatch(EntityEvent::SetVisible(false), 0.1);
        assert!(!model.needs_frame());
        model.dispatch(EntityEvent::SetVisible(true), 0.2);
        model.dispatch(EntityEvent::SetMotion(MotionMode::Static), 0.2);
        assert!(!model.needs_frame());
    }

    #[test]
    fn offline_clears_transient_work_and_recovers() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(
            EntityEvent::ToolStarted {
                id: "a",
                name: "get_cpu_info",
            },
            0.1,
        );
        model.dispatch(EntityEvent::SetConnection(Connection::Offline), 0.2);
        assert_eq!(model.sample(0.3).active_tools, 0);
        model.dispatch(EntityEvent::SetConnection(Connection::Online), 1.0);
        assert_eq!(model.sample(1.1).connection, Connection::Online);
    }

    #[test]
    fn permission_wait_does_not_destroy_tool_state() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(EntityEvent::RequestStarted, 0.1);
        model.dispatch(
            EntityEvent::ToolStarted {
                id: "write",
                name: "write_file",
            },
            0.2,
        );
        model.dispatch(EntityEvent::PermissionRequired, 0.3);
        let waiting = model.sample(0.4);
        assert_eq!(waiting.permission, PermissionState::Required);
        assert_eq!(waiting.active_tools, 1);
        model.dispatch(EntityEvent::PermissionResolved, 0.5);
        assert_eq!(model.sample(0.6).active_tools, 1);
    }

    #[test]
    fn failed_tool_then_failed_request_never_becomes_success() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(EntityEvent::RequestStarted, 0.1);
        model.dispatch(
            EntityEvent::ToolStarted {
                id: "a",
                name: "get_cpu_info",
            },
            0.2,
        );
        model.dispatch(EntityEvent::ToolFinished { id: "a", ok: false }, 0.3);
        model.dispatch(EntityEvent::RequestFailed { recoverable: true }, 0.4);
        assert!(matches!(
            model.sample(0.5).feedback,
            Some(FeedbackKind::Error { .. })
        ));
    }

    #[test]
    fn listening_transcribing_thinking_responding_sequence_is_stable() {
        let mut model = EntityModel::new(0.0);
        model.dispatch(EntityEvent::ListeningStarted, 0.1);
        assert_eq!(model.sample(0.2).activity, Activity::Listening);
        model.dispatch(EntityEvent::ListeningStopped, 0.3);
        assert_eq!(model.sample(0.4).activity, Activity::Transcribing);
        model.dispatch(EntityEvent::TranscriptionSubmitted, 0.5);
        assert_eq!(model.sample(0.6).activity, Activity::Thinking);
        model.dispatch(EntityEvent::TextDelta, 0.7);
        assert_eq!(model.sample(0.8).activity, Activity::Responding);
    }
}
