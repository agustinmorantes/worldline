use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct MonitoringSettings {
    pub version_cooldown_minutes: u32,
    pub snapshot_debounce_seconds: f32,
}

impl Default for MonitoringSettings {
    fn default() -> Self {
        Self {
            version_cooldown_minutes: 15,
            snapshot_debounce_seconds: 5.0,
        }
    }
}
