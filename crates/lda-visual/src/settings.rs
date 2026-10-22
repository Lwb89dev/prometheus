use std::fs;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::MotionMode;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VisualSettings {
    pub show_widget: bool,
    pub size: u32,
    pub intensity: f64,
    pub motion: MotionMode,
    pub voice_reactivity: bool,
    pub visual_effects: bool,
    pub startup_visible: bool,
    pub placement: Placement,
}

impl Default for VisualSettings {
    fn default() -> Self {
        Self {
            show_widget: true,
            size: 192,
            intensity: 0.8,
            motion: MotionMode::Full,
            voice_reactivity: false,
            visual_effects: true,
            startup_visible: true,
            placement: Placement::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Placement {
    /// Stable connector name, when the compositor exposes one.
    pub monitor: Option<String>,
    /// Wayland-safe fallback placement: `top_left`, `top_right`, `bottom_left`, or `bottom_right`.
    pub anchor: String,
    /// Normalized offset inside the monitor work area.
    pub offset_x: f64,
    pub offset_y: f64,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            monitor: None,
            anchor: "bottom_right".to_string(),
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VisualSettingsError {
    #[error("visual settings I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid visual settings: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid visual settings: {0}")]
    Invalid(String),
}

impl VisualSettings {
    pub fn validate(&mut self) -> Result<(), VisualSettingsError> {
        if !(96..=512).contains(&self.size) {
            return Err(VisualSettingsError::Invalid(
                "size must be between 96 and 512".into(),
            ));
        }
        if !self.intensity.is_finite() || !(0.0..=1.0).contains(&self.intensity) {
            return Err(VisualSettingsError::Invalid(
                "intensity must be between 0 and 1".into(),
            ));
        }
        if !matches!(
            self.placement.anchor.as_str(),
            "top_left" | "top_right" | "bottom_left" | "bottom_right"
        ) {
            return Err(VisualSettingsError::Invalid(
                "unsupported placement anchor".into(),
            ));
        }
        self.placement.offset_x = finite_clamp(self.placement.offset_x);
        self.placement.offset_y = finite_clamp(self.placement.offset_y);
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, VisualSettingsError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut value: Self = serde_json::from_slice(&fs::read(path)?)?;
        value.validate()?;
        Ok(value)
    }

    pub fn save(&self, path: &Path) -> Result<(), VisualSettingsError> {
        let mut checked = self.clone();
        checked.validate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&serde_json::to_vec_pretty(&checked)?)?;
        file.sync_all()?;
        fs::rename(tmp, path)?;
        Ok(())
    }
}

fn finite_clamp(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_clamp_position() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("visual.json");
        let mut settings = VisualSettings::default();
        settings.placement.offset_x = 99.0;
        settings.save(&path).unwrap();
        let loaded = VisualSettings::load(&path).unwrap();
        assert_eq!(loaded.placement.offset_x, 1.0);
        assert_eq!(loaded.size, 192);
    }

    #[test]
    fn invalid_animation_parameters_are_rejected() {
        let mut settings = VisualSettings {
            intensity: f64::NAN,
            ..VisualSettings::default()
        };
        assert!(settings.validate().is_err());
        settings = VisualSettings {
            size: 40,
            ..VisualSettings::default()
        };
        assert!(settings.validate().is_err());
    }
}
