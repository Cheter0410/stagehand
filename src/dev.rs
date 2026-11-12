//! Developer tools: for every app whose source is set up on this computer,
//! open a terminal that releases it (`release.sh`) or builds and installs it
//! from source - one app or all of them in a row.
//!
//! The work itself is done by each repository's own scripts; Stagehand only
//! finds the checkout, shows its state and opens the terminal.

use crate::catalog::AppDef;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Settings kept between starts.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// The developer section is shown.
    pub dev_mode: bool,
    /// App id -> folder of its source checkout (chosen by hand).
    pub repos: BTreeMap<String, String>,
}

fn settings_file() -> Option<PathBuf> {
    Some(crate::paths::config_dir()?.join("settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        settings_file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(f) = settings_file() else { return };
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(f, t);
        }
    }
}

/// Developer tools only exist where the repositories' bash scripts run.
pub fn available() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux"))
}

fn is_checkout(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Where a new checkout of `app` goes: next to the other ones if there are
/// any, else ~/Desktop/dev on a Mac and ~/dev elsewhere.
pub fn default_dir(app: &AppDef, others: &[PathBuf]) -> Option<PathBuf> {
    let dev = app.dev.as_ref()?;
    let parent = others
        .iter()
        .find_map(|p| p.parent().map(Path::to_path_buf))
        .or_else(|| {
            let home = dirs::home_dir()?;
            Some(if cfg!(target_os = "macos") {
                home.join("Desktop").join("dev")
            } else {
                home.join("dev")
            })
        })?;
    Some(parent.join(&dev.folder))
}

/// The checkout of `app`: the folder chosen in the settings, else the first
/// git checkout with the app's folder or repository name (any letter case)
/// in ~/Desktop/dev, ~/dev, ~/Developer, ~/src, ~/code or ~.
pub fn find_checkout(app: &AppDef, settings: &Settings) -> Option<PathBuf> {
    if let Some(p) = settings.repos.get(&app.id) {
        let p = PathBuf::from(p);
        return is_checkout(&p).then_some(p);
    }
    let dev = app.dev.as_ref()?;
    let repo_name = dev
        .repo
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let names = [dev.folder.to_lowercase(), repo_name];
    let home = dirs::home_dir()?;
    let bases = [
        home.join("Desktop").join("dev"),
        home.join("dev"),
        home.join("Developer"),
        home.join("src"),
        home.join("code"),
        home.clone(),
    ];
    for base in bases {
        let Ok(rd) = std::fs::read_dir(&base) else {
            continue;
        };
        let mut hits: Vec<PathBuf> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| names.contains(&n.to_lowercase()))
                    && is_checkout(p)
            })
            .collect();
        hits.sort();
        if let Some(p) = hits.into_iter().next() {
            return Some(p);
        }
    }
    None
}

/// State of a checkout, for the developer section.
#[derive(Clone, Debug, PartialEq)]
pub struct RepoState {
    pub branch: String,
    /// Changed tracked files (a release needs none).
    pub changes: usize,
    /// Commits on GitHub that are not here yet (as of the last fetch).
    pub behind: usize,
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

pub fn repo_state(dir: &Path) -> Option<RepoState> {
    let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let changes = git(dir, &["status", "--porcelain", "--untracked-files=no"])
        .map(|s| s.lines().count())
        .unwrap_or(0);
    let behind = git(dir, &["rev-list", "--count", "HEAD..@{u}"])
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Some(RepoState {
        branch,
        changes,
        behind,
    })
}

/// Quote for bash (single quotes).
pub fn quote(s: &str) -> String {
    q(s)
}

fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// One step of a terminal job: run `command` in `dir`, titled `name`.
pub struct Step {
    pub name: String,
    pub dir: PathBuf,
    pub command: String,
}

/// The bash script a terminal runs: the steps one after another (a failed
/// step does not stop the others), a summary, and the window stays open.
pub fn script(title: &str, steps: &[Step]) -> String {
    let mut s = String::from("#!/bin/bash\n");
    s.push_str("B=$'\\033[1m'; G=$'\\033[32m'; R=$'\\033[31m'; N=$'\\033[0m'\n");
    s.push_str(&format!("printf '%s%s%s\\n' \"$B\" {} \"$N\"\n", q(title)));
    s.push_str("failed=()\n");
    for st in steps {
        s.push_str(&format!(
            "printf '\\n%s▶ %s%s\\n' \"$B\" {} \"$N\"\n",
            q(&st.name)
        ));
        s.push_str(&format!(
            "( cd {} && {} ) || failed+=({})\n",
            q(&st.dir.display().to_string()),
            st.command,
            q(&st.name)
        ));
    }
    if steps.len() > 1 {
        s.push_str("printf '\\n%sSummary%s\\n' \"$B\" \"$N\"\n");
        for st in steps {
            s.push_str(&format!(
                "if printf '%s\\n' \"${{failed[@]}}\" | grep -qxF {n}; then printf '  %s✗%s %s\\n' \"$R\" \"$N\" {n}; else printf '  %s✓%s %s\\n' \"$G\" \"$N\" {n}; fi\n",
                n = q(&st.name)
            ));
        }
    }
    s.push_str("printf '\\nDone. Press Enter to close this window. '\nread -r _\n");
    s
}

/// Linux terminals and how to make them run a script.
const LINUX_TERMINALS: &[(&str, &[&str])] = &[
    ("kitty", &[]),
    ("alacritty", &["-e"]),
    ("foot", &[]),
    ("wezterm", &["start", "--"]),
    ("ghostty", &["-e"]),
    ("konsole", &["-e"]),
    ("gnome-terminal", &["--"]),
    ("kgx", &["--"]),
    ("xfce4-terminal", &["-x"]),
    ("tilix", &["-e"]),
    ("terminator", &["-x"]),
    ("x-terminal-emulator", &["-e"]),
    ("xterm", &["-e"]),
];

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

/// Open a terminal window running `steps`.
pub fn open_terminal(title: &str, steps: &[Step]) -> Result<(), String> {
    let file = std::env::temp_dir().join(format!(
        "stagehand-{}-{}.sh",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    std::fs::write(&file, script(title, steps)).map_err(|e| e.to_string())?;
    let path = file.display().to_string();
    if cfg!(target_os = "macos") {
        let cmd = format!("bash {}", q(&path));
        let apple = format!(
            "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
            cmd.replace('\\', "\\\\").replace('"', "\\\"")
        );
        return Command::new("osascript")
            .args(["-e", &apple])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open Terminal: {e}"));
    }
    // $TERMINAL first (the user's choice), then the usual ones.
    if let Ok(t) = std::env::var("TERMINAL") {
        let t = t.trim().to_string();
        if !t.is_empty() && on_path(&t) {
            let flag = LINUX_TERMINALS
                .iter()
                .find(|(n, _)| *n == t)
                .map_or(&["-e"][..], |(_, f)| f);
            return Command::new(&t)
                .args(flag)
                .args(["bash", &path])
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("Could not open {t}: {e}"));
        }
    }
    for (bin, flag) in LINUX_TERMINALS {
        if on_path(bin) {
            return Command::new(bin)
                .args(*flag)
                .args(["bash", &path])
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("Could not open {bin}: {e}"));
        }
    }
    Err("No terminal found (e.g. kitty, alacritty, konsole, gnome-terminal). Set the TERMINAL environment variable to choose one.".into())
}

/// The command that installs `app` from source on this system.
pub fn install_command(app: &AppDef) -> Option<String> {
    let d = app.dev.as_ref()?;
    if cfg!(target_os = "macos") {
        d.install_macos.clone()
    } else {
        d.install_linux.clone()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn script_runs_steps_and_summarises() {
        let steps = vec![
            Step {
                name: "Good".into(),
                dir: PathBuf::from("/tmp"),
                command: "true".into(),
            },
            Step {
                name: "Bad 'one'".into(),
                dir: PathBuf::from("/tmp"),
                command: "false".into(),
            },
        ];
        let s = script("Test", &steps).replace("read -r _", "true");
        let out = Command::new("bash").args(["-c", &s]).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains("▶ Good") && text.contains("▶ Bad 'one'"),
            "{text}"
        );
        assert!(text.contains("✓\u{1b}[0m Good"), "{text}");
        assert!(text.contains("✗\u{1b}[0m Bad 'one'"), "{text}");
    }

    #[test]
    fn settings_round_trip() {
        let mut s = Settings {
            dev_mode: true,
            ..Default::default()
        };
        s.repos.insert("showmaster".into(), "/x/y".into());
        let t = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&t).unwrap(), s);
    }
}
