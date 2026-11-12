//! The apps Stagehand knows: built in (`apps.json` in the repository), and
//! refreshed from `apps.json` in the releases repository, so a new app shows
//! up without a new Stagehand. The last downloaded catalog is kept for
//! starting without internet.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where the live catalog lives (the default branch of the releases repository).
pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/Cheter0410/stagehand-releases/main/apps.json";

const BUILT_IN: &str = include_str!("../apps.json");

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Catalog {
    pub apps: Vec<AppDef>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AppDef {
    /// Stable identifier (also the name of its built-in icon).
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tagline: String,
    /// Accent colour, `#rrggbb`.
    #[serde(default)]
    pub color: String,
    /// GitHub repository with the releases (`owner/name`).
    pub releases: String,
    /// Icon for apps added after this build (PNG).
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub macos: Option<MacDef>,
    #[serde(default)]
    pub windows: Option<WinDef>,
    #[serde(default)]
    pub linux: Option<LinuxDef>,
    /// Where the source lives and how it is released / installed from source
    /// (developer section).
    #[serde(default)]
    pub dev: Option<DevDef>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct DevDef {
    /// GitHub repository with the source (`owner/name`).
    pub repo: String,
    /// Usual name of the checkout folder.
    pub folder: String,
    /// Command (in the checkout) that releases a new version.
    pub release: Option<String>,
    /// Commands that build and install from source.
    pub install_macos: Option<String>,
    pub install_linux: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MacDef {
    /// File name ending of the disk image in the release (`.dmg`).
    pub asset_suffix: String,
    /// Bundle name inside the disk image and in /Applications.
    pub app: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WinDef {
    #[serde(default)]
    pub asset_prefix: String,
    pub asset_suffix: String,
    /// Inno Setup `AppId` (with braces): finds the installed version.
    pub inno_app_id: String,
    /// Program file in the install folder.
    pub exe: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LinuxDef {
    /// File name ending of the portable archive (`-linux-x86_64.tar.gz`).
    pub asset_suffix: String,
    /// Program name (in the archive, and on PATH for system installs).
    pub bin: String,
}

impl Catalog {
    pub fn built_in() -> Self {
        serde_json::from_str(BUILT_IN).unwrap_or(Catalog { apps: Vec::new() })
    }

    /// The last catalog downloaded, else the built-in one. Apps of the
    /// built-in catalog missing from a downloaded one stay (an old live
    /// catalog never hides an app this build knows).
    pub fn load() -> Self {
        let built = Self::built_in();
        let cached = cache_file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str::<Catalog>(&t).ok());
        match cached {
            Some(c) => c.merged_with(&built),
            None => built,
        }
    }

    /// Download the live catalog (and keep it for next time).
    pub fn fetch() -> Result<Self, String> {
        let text = crate::github::get_text(CATALOG_URL)?.ok_or("no live catalog published yet")?;
        let c: Catalog = serde_json::from_str(&text).map_err(|e| format!("catalog: {e}"))?;
        if let Some(f) = cache_file() {
            if let Some(d) = f.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(f, &text);
        }
        Ok(c.merged_with(&Self::built_in()))
    }

    fn merged_with(mut self, other: &Catalog) -> Self {
        // Developer details this build knows fill in an older live catalog.
        for a in &mut self.apps {
            if a.dev.is_none() {
                a.dev = other
                    .apps
                    .iter()
                    .find(|b| b.id == a.id)
                    .and_then(|b| b.dev.clone());
            }
        }
        for a in &other.apps {
            if !self.apps.iter().any(|b| b.id == a.id) {
                self.apps.push(a.clone());
            }
        }
        self
    }
}

fn cache_file() -> Option<PathBuf> {
    Some(crate::paths::config_dir()?.join("apps.json"))
}

/// Built-in icon of an app, if this build has one.
pub fn built_in_icon(id: &str) -> Option<&'static [u8]> {
    match id {
        "showmaster" => Some(include_bytes!("../assets/apps/showmaster.png")),
        "beamweave" => Some(include_bytes!("../assets/apps/beamweave.png")),
        _ => None,
    }
}

/// `#rrggbb` to RGB.
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn built_in_catalog_parses_and_has_icons() {
        let c = Catalog::built_in();
        assert!(c.apps.len() >= 2);
        for a in &c.apps {
            assert!(built_in_icon(&a.id).is_some(), "{} has an icon", a.id);
            assert!(parse_color(&a.color).is_some());
            assert!(a.releases.contains('/'));
            let w = a.windows.as_ref().unwrap();
            assert!(w.inno_app_id.starts_with('{') && w.inno_app_id.ends_with('}'));
        }
    }

    #[test]
    fn a_live_catalog_never_hides_a_built_in_app() {
        let live = Catalog {
            apps: vec![AppDef {
                id: "new".into(),
                name: "New".into(),
                tagline: String::new(),
                color: String::new(),
                releases: "a/b".into(),
                icon_url: None,
                macos: None,
                windows: None,
                linux: None,
                dev: None,
            }],
        };
        let m = live.merged_with(&Catalog::built_in());
        assert_eq!(m.apps[0].id, "new");
        assert!(m.apps.iter().any(|a| a.id == "showmaster"));
        let old = Catalog {
            apps: vec![AppDef {
                dev: None,
                ..Catalog::built_in().apps[0].clone()
            }],
        };
        assert!(old.merged_with(&Catalog::built_in()).apps[0].dev.is_some());
    }
}
