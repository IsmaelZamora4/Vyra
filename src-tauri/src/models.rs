use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_PROFILES: usize = 128;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BackendPreference {
    #[default]
    Automatic,
    Nvidia,
    Amd,
    Windows,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub brightness: i16,
    pub contrast: i16,
    pub saturation: i16,
    pub gamma: f32,
    pub hue: i16,
    pub sharpness: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            brightness: 0,
            contrast: 0,
            saturation: 0,
            gamma: 1.0,
            hue: 0,
            sharpness: 0,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(-100..=100).contains(&self.brightness)
            || !(-100..=100).contains(&self.contrast)
            || !(-100..=100).contains(&self.saturation)
            || !(-180..=180).contains(&self.hue)
            || !(0.5..=2.0).contains(&self.gamma)
            || self.sharpness > 100
        {
            return Err("Ajustes fuera de rango.".into());
        }
        if !self.gamma.is_finite() {
            return Err("Gamma debe ser un número finito.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub builtin: bool,
    pub settings: Settings,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileConfig {
    pub schema_version: u32,
    pub selected_profile_id: String,
    #[serde(default)]
    pub backend_preference: BackendPreference,
    pub profiles: Vec<Profile>,
}

impl ProfileConfig {
    pub fn defaults() -> Self {
        let profiles = vec![
            Profile {
                id: "builtin-competitive".into(),
                name: "Competitivo".into(),
                builtin: true,
                settings: Settings::default(),
            },
            Profile {
                id: "builtin-cinematic".into(),
                name: "Cinematográfico".into(),
                builtin: true,
                settings: Settings {
                    contrast: 8,
                    saturation: 6,
                    gamma: 1.0,
                    ..Settings::default()
                },
            },
            Profile {
                id: "builtin-vibrant".into(),
                name: "Vibrante".into(),
                builtin: true,
                settings: Settings {
                    saturation: 24,
                    hue: 0,
                    gamma: 1.0,
                    ..Settings::default()
                },
            },
            Profile {
                id: "builtin-night".into(),
                name: "Nocturno".into(),
                builtin: true,
                settings: Settings {
                    brightness: -10,
                    contrast: 12,
                    gamma: 1.0,
                    ..Settings::default()
                },
            },
        ];
        Self {
            schema_version: SCHEMA_VERSION,
            selected_profile_id: profiles[0].id.clone(),
            backend_preference: BackendPreference::Automatic,
            profiles,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "Versión de configuración no compatible: {}.",
                self.schema_version
            ));
        }
        if self.profiles.is_empty() || self.profiles.len() > MAX_PROFILES {
            return Err(format!(
                "La configuración debe tener entre 1 y {MAX_PROFILES} perfiles."
            ));
        }
        if !self
            .profiles
            .iter()
            .any(|profile| profile.id == self.selected_profile_id)
        {
            return Err("El perfil seleccionado no existe.".into());
        }
        let mut ids = HashSet::with_capacity(self.profiles.len());
        for profile in &self.profiles {
            if profile.id.trim().is_empty() || !ids.insert(&profile.id) {
                return Err("Los identificadores de perfil deben ser únicos y no vacíos.".into());
            }
            validate_profile(profile)?;
        }
        Ok(())
    }
}

pub fn validate_profile(profile: &Profile) -> Result<(), String> {
    let name = profile.name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("El nombre debe tener entre 1 y 40 caracteres.".into());
    }
    profile.settings.validate()
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileExport {
    pub schema_version: u32,
    pub profiles: Vec<Profile>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_accept_supported_endpoints() {
        Settings {
            brightness: -100,
            contrast: 100,
            saturation: -100,
            gamma: 0.5,
            hue: -180,
            sharpness: 100,
        }
        .validate()
        .unwrap();
        Settings {
            brightness: 100,
            contrast: -100,
            saturation: 100,
            gamma: 2.0,
            hue: 180,
            sharpness: 0,
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn settings_reject_out_of_range_values() {
        assert!(
            Settings {
                brightness: 101,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                contrast: -101,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                saturation: 101,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                hue: 181,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                gamma: f32::NAN,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                gamma: 2.01,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                sharpness: 101,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn profile_config_round_trips_json_and_validates() {
        let mut config = ProfileConfig::defaults();
        config.backend_preference = BackendPreference::Nvidia;
        let json = serde_json::to_string(&config).unwrap();
        let decoded: ProfileConfig = serde_json::from_str(&json).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded.selected_profile_id, config.selected_profile_id);
        assert_eq!(decoded.profiles.len(), 4);
        assert_eq!(decoded.backend_preference, BackendPreference::Nvidia);
    }

    #[test]
    fn old_profile_config_defaults_to_automatic_backend() {
        let mut value = serde_json::to_value(ProfileConfig::defaults()).unwrap();
        value.as_object_mut().unwrap().remove("backendPreference");
        let decoded: ProfileConfig = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.backend_preference, BackendPreference::Automatic);
    }

    #[test]
    fn backend_preference_serializes_as_stable_lower_camel_case() {
        for (preference, expected) in [
            (BackendPreference::Automatic, "\"automatic\""),
            (BackendPreference::Nvidia, "\"nvidia\""),
            (BackendPreference::Amd, "\"amd\""),
            (BackendPreference::Windows, "\"windows\""),
        ] {
            assert_eq!(serde_json::to_string(&preference).unwrap(), expected);
        }
    }
}
