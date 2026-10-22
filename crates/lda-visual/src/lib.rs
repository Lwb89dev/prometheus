//! Native, state-driven visual presence for Prometeus.
//!
//! The model is deliberately independent from GTK. The UI forwards verified
//! daemon and input events into [`EntityModel`], while [`EntityWidget`] renders
//! a procedural scene from a monotonic timestamp.

#![forbid(unsafe_code)]

mod bridge;
mod model;
mod scene;
mod settings;
mod widget;

pub use bridge::DaemonVisualEvent;
pub use model::{
    Activity, Connection, EntityEvent, EntityModel, FeedbackKind, MotionMode, PermissionState,
    ToolCategory, VisualStatus,
};
pub use scene::{EnergyDirection, SceneFrame};
pub use settings::{Placement, VisualSettings, VisualSettingsError};
pub use widget::{EntityCallbacks, EntityWidget};
