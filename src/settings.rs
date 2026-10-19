//! Preferences that outlive a session, kept in `settings.json` beside the projects directory.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use egui::Color32;
use serde::{Deserialize, Serialize};

use crate::ai::Service;
use crate::output::DeviceProfile;
use crate::palette::Recents;
use crate::units::Units;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub units: Units,
    /// Open cutters, in the order the machine modal lists them.
    pub devices: Vec<DeviceProfile>,
    /// The profile the Make flow starts on; `None` means the simulated machine.
    pub last_device: Option<u64>,
    /// The command palette's runs, newest first.
    pub palette_recent: Recents,
    /// Who Create AI asks and which model (spec 43). API keys are never kept here.
    pub ai: AiSettings,
    /// The last colours applied in any project, newest first (spec 56). Session spanning, not per project.
    pub recent_colours: RecentColours,
    /// The user's own saved swatches (spec 56), in the order the picker shows them.
    pub swatches: Vec<Swatch>,
}

/// The last colours the user applied, newest first, at most [`RECENT_COLOURS_CAP`], each once.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecentColours(#[serde(with = "colours_serde")] Vec<Color32>);

/// `Vec<Color32>` as a list of `[r, g, b, a]`, matching how a shape's fill is stored.
mod colours_serde {
    use egui::Color32;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(v: &[Color32], s: S) -> Result<S::Ok, S::Error> {
        v.iter().map(Color32::to_array).collect::<Vec<_>>().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Color32>, D::Error> {
        Ok(<Vec<[u8; 4]>>::deserialize(d)?.into_iter().map(|[r, g, b, a]| Color32::from_rgba_premultiplied(r, g, b, a)).collect())
    }
}

pub const RECENT_COLOURS_CAP: usize = 12;

impl RecentColours {
    /// Moves `colour` to the front, dropping any earlier use of it and trimming to the cap.
    pub fn record(&mut self, colour: Color32) {
        self.0.retain(|c| *c != colour);
        self.0.insert(0, colour);
        self.0.truncate(RECENT_COLOURS_CAP);
    }

    pub fn iter(&self) -> impl Iterator<Item = Color32> + '_ {
        self.0.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// One saved swatch: a name the user gave it and the colour it keeps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub name: String,
    #[serde(with = "crate::design::color_serde")]
    pub colour: Color32,
}

/// Where a local model server listens by default: Ollama's OpenAI-compatible endpoint.
pub const DEFAULT_LOCAL_ADDRESS: &str = "http://localhost:11434/v1";

/// The one place Create AI's default models are named. OpenRouter and local servers host whatever the
/// user picks, so they have none.
pub fn default_ai_model(service: Service) -> &'static str {
    match service {
        Service::Anthropic => "claude-opus-5-5",
        Service::OpenAi => "gpt-6-astra",
        Service::OpenRouter | Service::Local => "",
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub service: Service,
    /// Each service's model, once the user changes it from [`default_ai_model`].
    pub models: BTreeMap<Service, String>,
    /// The local server's base URL, `/v1` included.
    pub local_address: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self { service: Service::default(), models: BTreeMap::new(), local_address: DEFAULT_LOCAL_ADDRESS.into() }
    }
}

impl AiSettings {
    pub fn model(&self, service: Service) -> &str {
        self.models.get(&service).map_or_else(|| default_ai_model(service), String::as_str).trim()
    }

    pub fn model_mut(&mut self, service: Service) -> &mut String {
        self.models.entry(service).or_insert_with(|| default_ai_model(service).into())
    }
}

impl Settings {
    fn path(projects_dir: &Path) -> PathBuf {
        projects_dir.with_file_name("settings.json")
    }

    /// The saved settings, or the defaults when there are none yet or the file is unreadable.
    pub fn load(projects_dir: &Path) -> Self {
        std::fs::read_to_string(Self::path(projects_dir)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, projects_dir: &Path) -> io::Result<()> {
        let path = Self::path(projects_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::store::write_atomically(&path, serde_json::to_string_pretty(self).map_err(io::Error::other)?.as_bytes())
    }

    pub fn last_profile(&self) -> Option<&DeviceProfile> {
        self.devices.iter().find(|d| Some(d.id) == self.last_device)
    }

    /// Saves `profile` over the one with its id, or adds it, under a fresh id when it has none (0).
    /// Returns the id it is saved under.
    pub fn upsert(&mut self, mut profile: DeviceProfile) -> u64 {
        if let Some(slot) = self.devices.iter_mut().find(|d| d.id == profile.id && profile.id != 0) {
            *slot = profile;
            return slot.id;
        }
        if profile.id == 0 {
            profile.id = next_id(&self.devices);
        }
        self.devices.push(profile);
        self.devices.last().map_or(0, |d| d.id)
    }

    pub fn remove(&mut self, id: u64) {
        self.devices.retain(|d| d.id != id);
        if self.last_device == Some(id) {
            self.last_device = None;
        }
    }
}

/// An id no saved profile has.
pub fn next_id(devices: &[DeviceProfile]) -> u64 {
    devices.iter().map(|d| d.id).max().unwrap_or(0) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_survive_a_save_and_load_beside_the_projects() {
        let root = tempfile::tempdir().unwrap();
        let projects = root.path().join("projects");
        assert_eq!(Settings::load(&projects), Settings::default(), "no file yet");
        Settings { units: Units::Metric, ..Settings::default() }.save(&projects).unwrap();
        assert!(root.path().join("settings.json").exists());
        assert!(!projects.exists(), "the projects directory holds only projects");
        assert_eq!(Settings::load(&projects).units, Units::Metric);
    }

    #[test]
    fn unreadable_settings_fall_back_to_defaults() {
        let root = tempfile::tempdir().unwrap();
        let projects = root.path().join("projects");
        std::fs::write(root.path().join("settings.json"), "{ not json").unwrap();
        assert_eq!(Settings::load(&projects), Settings::default());
        std::fs::write(root.path().join("settings.json"), "{}").unwrap();
        assert_eq!(Settings::load(&projects), Settings::default(), "missing fields take their defaults");
    }

    #[test]
    fn device_profiles_round_trip_and_old_settings_load_with_none() {
        let root = tempfile::tempdir().unwrap();
        let projects = root.path().join("projects");
        let mut settings = Settings { units: Units::Metric, ..Settings::default() };
        let plotter = settings.upsert(DeviceProfile::grbl_pen_plotter());
        let cutter = settings.upsert(DeviceProfile { port: Some("/dev/cu.usbserial-1".into()), ..DeviceProfile::hpgl_vinyl_cutter() });
        assert_ne!(plotter, cutter, "each new profile gets its own id");
        settings.last_device = Some(cutter);
        settings.save(&projects).unwrap();
        let loaded = Settings::load(&projects);
        assert_eq!(loaded, settings);
        assert_eq!(loaded.last_profile().map(|d| d.name.as_str()), Some("HPGL vinyl cutter"));

        std::fs::write(root.path().join("settings.json"), r#"{ "units": "Metric" }"#).unwrap();
        let old = Settings::load(&projects);
        assert_eq!((old.units, old.devices.len(), old.last_device), (Units::Metric, 0, None));
    }

    #[test]
    fn upsert_replaces_a_saved_profile_in_place_and_remove_forgets_it() {
        let mut settings = Settings::default();
        let id = settings.upsert(DeviceProfile::grbl_laser());
        let renamed = DeviceProfile { id, name: "Desk laser".into(), ..DeviceProfile::grbl_laser() };
        assert_eq!(settings.upsert(renamed), id);
        assert_eq!(settings.devices.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["Desk laser"]);
        settings.last_device = Some(id);
        settings.remove(id);
        assert_eq!((settings.devices.len(), settings.last_device), (0, None));
    }

    #[test]
    fn palette_recents_survive_a_save_and_load_and_cap_at_fifty() {
        let root = tempfile::tempdir().unwrap();
        let projects = root.path().join("projects");
        let mut settings = Settings::default();
        for i in 0..60 {
            settings.palette_recent.record(&format!("cmd:{i}"), 1_000 + i);
        }
        settings.save(&projects).unwrap();
        let loaded = Settings::load(&projects);
        assert_eq!(loaded, settings);
        assert_eq!(loaded.palette_recent.len(), crate::palette::RECENT_CAP);
        assert_eq!(loaded.palette_recent.latest().next(), Some("cmd:59"));
    }

    #[test]
    fn the_create_ai_service_and_models_round_trip_and_default_for_old_settings() {
        let root = tempfile::tempdir().unwrap();
        let projects = root.path().join("projects");
        std::fs::write(root.path().join("settings.json"), r#"{ "units": "Metric" }"#).unwrap();
        let old = Settings::load(&projects).ai;
        assert_eq!((old.service, old.model(Service::Anthropic), old.local_address.as_str()), (Service::Anthropic, "claude-opus-5-5", DEFAULT_LOCAL_ADDRESS));
        assert_eq!(old.model(Service::Local), "", "a local model has to be named");
        let mut ai = AiSettings { service: Service::Local, local_address: "http://localhost:1234/v1".into(), ..AiSettings::default() };
        *ai.model_mut(Service::Local) = "qwen3".into();
        *ai.model_mut(Service::OpenRouter) = "anthropic/claude-opus-5.5".into();
        Settings { ai: ai.clone(), ..Settings::default() }.save(&projects).unwrap();
        let text = std::fs::read_to_string(root.path().join("settings.json")).unwrap();
        assert!(text.contains("\"Local\": \"qwen3\""), "{text}");
        assert_eq!(Settings::load(&projects).ai, ai);
    }

    #[test]
    fn recent_colours_cap_at_twelve_newest_first_with_no_duplicates() {
        let mut recent = RecentColours::default();
        for i in 0..20u8 {
            recent.record(Color32::from_rgb(i, 0, 0));
        }
        assert_eq!(recent.len(), RECENT_COLOURS_CAP);
        assert_eq!(recent.iter().next(), Some(Color32::from_rgb(19, 0, 0)), "newest first");
        recent.record(Color32::from_rgb(19, 0, 0));
        assert_eq!(recent.len(), RECENT_COLOURS_CAP, "recording the same colour again adds no row");
        assert_eq!(recent.iter().next(), Some(Color32::from_rgb(19, 0, 0)));
    }

    #[test]
    fn swatches_and_recent_colours_round_trip_through_settings_json() {
        let root = tempfile::tempdir().unwrap();
        let projects = root.path().join("projects");
        let mut settings = Settings {
            swatches: vec![
                Swatch { name: "Navy".into(), colour: Color32::from_rgb(0x1C, 0x2E, 0x6B) },
                Swatch { name: "Brand red".into(), colour: Color32::from_rgb(0xD0, 0x20, 0x2E) },
            ],
            ..Settings::default()
        };
        settings.recent_colours.record(Color32::from_rgb(0x70, 0x48, 0xE8));
        settings.save(&projects).unwrap();
        let loaded = Settings::load(&projects);
        assert_eq!(loaded, settings);
        assert_eq!(loaded.swatches[0].name, "Navy");
        assert_eq!(loaded.recent_colours.iter().next(), Some(Color32::from_rgb(0x70, 0x48, 0xE8)));

        std::fs::write(root.path().join("settings.json"), r#"{ "units": "Metric" }"#).unwrap();
        let old = Settings::load(&projects);
        assert!(old.swatches.is_empty() && old.recent_colours.is_empty(), "settings from before spec 56 load empty");
    }

    #[test]
    fn settings_from_before_the_palette_load_with_no_recents() {
        let root = tempfile::tempdir().unwrap();
        let projects = root.path().join("projects");
        std::fs::write(root.path().join("settings.json"), r#"{ "units": "Metric" }"#).unwrap();
        let loaded = Settings::load(&projects);
        assert_eq!((loaded.units, loaded.palette_recent.len()), (Units::Metric, 0));
    }
}
