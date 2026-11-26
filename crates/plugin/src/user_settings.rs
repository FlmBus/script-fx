//! Per-user shell settings: `<user data dir>/ScriptFX/settings.json`. Not plugin state, which
//! travels inside the DAW project: these follow the user across projects and instances.
//!
//! Unknown keys are kept on save (an older build doesn't drop a newer build's settings).
//! A missing or broken file means defaults.

use std::path::PathBuf;

use serde_json::{Map, Value, json};

/// UI scale steps: 50 % to 200 % in 25 % steps.
pub const UI_SCALE_MIN: f32 = 0.5;
pub const UI_SCALE_MAX: f32 = 2.0;
pub const UI_SCALE_STEP: f32 = 0.25;

/// The shell's colour mode. `System` follows the OS setting.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

impl ThemeMode {
    pub const ALL: [ThemeMode; 3] = [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark];

    fn key(self) -> &'static str {
        match self {
            ThemeMode::System => "system",
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
        }
    }

    fn from_key(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == s)
    }
}

/// How much the editor spends on looking good. Lower levels give a weak machine its frames back;
/// what each one turns off is in `shell::quality`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Quality {
    #[default]
    High,
    Medium,
    Low,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::High, Quality::Medium, Quality::Low];

    pub fn label(self) -> &'static str {
        match self {
            Quality::High => "High",
            Quality::Medium => "Medium",
            Quality::Low => "Low",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Quality::High => "high",
            Quality::Medium => "medium",
            Quality::Low => "low",
        }
    }

    fn from_key(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|q| q.key() == s)
    }

    /// Multisampling (iced's MSAA ×4). The single biggest thing the GPU does for us: measured at
    /// 3× the GPU time per frame on a software renderer (`docs/research/render-performance.md`).
    /// Only read when a window opens.
    pub fn msaa(self) -> bool {
        self == Quality::High
    }

    /// The renderer's blur pass (the block modal's zoom and backdrop).
    pub fn blur(self) -> bool {
        self != Quality::Low
    }

    /// Whether the patch's own UI keeps moving. Off, its components stop being re-rendered and
    /// re-uploaded every frame.
    pub fn animations(self) -> bool {
        self != Quality::Low
    }
}

/// Frames per second the editor aims for. The editor draws on a clock, so this is a limiter: it
/// scales down every per-frame cost there is. (It idles below this when nothing moves.)
pub const FRAME_RATES: [u32; 3] = [60, 30, 20];

/// Clamps to one of [`FRAME_RATES`].
pub fn snap_frame_rate(v: u32) -> u32 {
    FRAME_RATES.into_iter().min_by_key(|r| r.abs_diff(v)).unwrap_or(60)
}

/// Who answers in the chat.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Provider {
    /// Nothing leaves the machine until the user picks one.
    #[default]
    Off,
    /// The user's installed, logged-in Claude Code.
    Claude,
    /// An OpenAI-compatible API; its key is in the OS keychain.
    Api,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Off, Provider::Claude, Provider::Api];

    fn key(self) -> &'static str {
        match self {
            Provider::Off => "off",
            Provider::Claude => "claude",
            Provider::Api => "api",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ai {
    pub provider: Provider,
    /// Per provider: the names have nothing in common.
    pub claude_model: String,
    pub api_model: String,
    pub base_url: String,
    /// `CLAUDE_CONFIG_DIR` for hosts that don't have it in their environment. Empty = unset.
    pub claude_config_dir: String,
}

impl Default for Ai {
    fn default() -> Self {
        Self {
            provider: Provider::Off,
            claude_model: String::new(),
            api_model: String::new(),
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            claude_config_dir: String::new(),
        }
    }
}

impl Ai {
    fn read(map: &Map<String, Value>) -> Self {
        let d = Self::default();
        let v = map.get("ai");
        let get = |k: &str, default: String| v.and_then(|v| v.get(k)).and_then(Value::as_str).map_or(default, String::from);
        let provider = get("provider", String::new());
        Self {
            provider: Provider::ALL.into_iter().find(|p| p.key() == provider).unwrap_or_default(),
            claude_model: get("claude_model", d.claude_model),
            api_model: get("api_model", d.api_model),
            base_url: get("base_url", d.base_url),
            claude_config_dir: get("claude_config_dir", d.claude_config_dir),
        }
    }

    fn write(&self, map: &mut Map<String, Value>) {
        map.insert("ai".into(), json!({
            "provider": self.provider.key(),
            "claude_model": self.claude_model,
            "api_model": self.api_model,
            "base_url": self.base_url,
            "claude_config_dir": self.claude_config_dir,
        }));
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserSettings {
    /// Size of the whole plugin window (1.0 = 1040×640 logical px).
    pub ui_scale: f32,
    pub theme: ThemeMode,
    /// What the editor is allowed to spend per frame (`shell::quality`).
    pub quality: Quality,
    /// Frames per second the editor aims for, one of [`FRAME_RATES`].
    pub frame_rate: u32,
    pub ai: Ai,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self { ui_scale: 1.0, theme: ThemeMode::System, quality: Quality::default(), frame_rate: 60, ai: Ai::default() }
    }
}

impl UserSettings {
    pub fn load() -> Self {
        let Some(map) = read() else { return Self::default() };
        let ui_scale = map.get("ui_scale").and_then(Value::as_f64).map_or(1.0, |v| snap_ui_scale(v as f32));
        let theme = map.get("theme").and_then(Value::as_str).and_then(ThemeMode::from_key).unwrap_or_default();
        let quality = map.get("quality").and_then(Value::as_str).and_then(Quality::from_key).unwrap_or_default();
        let frame_rate = map.get("frame_rate").and_then(Value::as_u64).map_or(60, |v| snap_frame_rate(v as u32));
        Self { ui_scale, theme, quality, frame_rate, ai: Ai::read(&map) }
    }

    pub fn save(&self) {
        let mut map = read().unwrap_or_default();
        map.insert("ui_scale".into(), json!(self.ui_scale));
        map.insert("theme".into(), json!(self.theme.key()));
        map.insert("quality".into(), json!(self.quality.key()));
        map.insert("frame_rate".into(), json!(self.frame_rate));
        self.ai.write(&mut map);
        let Some(path) = path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default();
        // Write + rename: two instances saving at once can't leave a half-written file.
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// Clamps to the allowed range and rounds to the nearest step.
pub fn snap_ui_scale(v: f32) -> f32 {
    if !v.is_finite() {
        return 1.0;
    }
    ((v / UI_SCALE_STEP).round() * UI_SCALE_STEP).clamp(UI_SCALE_MIN, UI_SCALE_MAX)
}

fn path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("ScriptFX").join("settings.json"))
}

fn read() -> Option<Map<String, Value>> {
    let text = std::fs::read_to_string(path()?).ok()?;
    match serde_json::from_str(&text).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_round_trips() {
        let ai = Ai { provider: Provider::Api, api_model: "m".into(), ..Default::default() };
        let mut map = Map::new();
        assert_eq!(Ai::read(&map), Ai::default());
        ai.write(&mut map);
        assert_eq!(Ai::read(&map), ai);
    }


    #[test]
    fn quality_and_frame_rate_round_trip() {
        let mut map = Map::new();
        // An empty (or older) settings file means the defaults.
        assert_eq!(map.get("quality").and_then(Value::as_str).and_then(Quality::from_key), None);
        map.insert("quality".into(), json!(Quality::Low.key()));
        assert_eq!(Quality::from_key(map["quality"].as_str().unwrap()), Some(Quality::Low));
        assert_eq!(snap_frame_rate(59), 60);
        assert_eq!(snap_frame_rate(31), 30);
        assert_eq!(snap_frame_rate(1), 20);
        assert_eq!(snap_frame_rate(1000), 60);
    }

    #[test]
    fn levels_turn_the_right_things_off() {
        assert!(Quality::High.msaa() && Quality::High.blur() && Quality::High.animations());
        assert!(!Quality::Medium.msaa() && Quality::Medium.blur() && Quality::Medium.animations());
        assert!(!Quality::Low.msaa() && !Quality::Low.blur() && !Quality::Low.animations());
    }

    #[test]
    fn ui_scale_snaps_to_steps() {
        assert_eq!(snap_ui_scale(1.1), 1.0);
        assert_eq!(snap_ui_scale(1.2), 1.25);
        assert_eq!(snap_ui_scale(0.1), 0.5);
        assert_eq!(snap_ui_scale(9.0), 2.0);
        assert_eq!(snap_ui_scale(f32::NAN), 1.0);
    }
}
