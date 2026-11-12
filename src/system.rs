//! What differs per operating system: which release file fits, where an app
//! is installed (and which version), installing it and starting it.
//!
//! All apps go into one folder per system, next to Stagehand itself:
//!
//! * macOS: the `.dmg`; the app goes to /Applications/Stagehand (or
//!   ~/Applications/Stagehand when /Applications is not writable). Version
//!   from the bundle's Info.plist. A copy found in the old place
//!   (/Applications) is moved there with the next install or update.
//! * Windows: the Inno Setup installer, run silently for the current user,
//!   into %LOCALAPPDATA%\Programs\Stagehand\<App>. Version and folder from the
//!   installer's uninstall entry in the registry; an older install elsewhere
//!   is removed after the new one is in place.
//! * Linux: the portable `.tar.gz`, unpacked to ~/.local/share/stagehand/apps
//!   with a menu entry and a link in ~/.local/bin. An app installed by the
//!   system (e.g. the Arch package) is found on PATH and started too.
//!
//! A running app is never replaced: a show may be going on.

use crate::catalog::AppDef;
use crate::github::{Asset, Release};
use std::path::{Path, PathBuf};
use std::process::Command;

/// An installed app.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    /// Unknown when the app is there but does not tell its version.
    pub version: Option<String>,
    /// What to start: an `.app` bundle (macOS) or a program file.
    pub path: PathBuf,
}

/// Whether this system can install `app` from its releases at all.
pub fn supported(app: &AppDef) -> bool {
    if cfg!(target_os = "macos") {
        app.macos.is_some()
    } else if cfg!(target_os = "windows") {
        app.windows.is_some()
    } else {
        app.linux.is_some()
    }
}

/// The release file for this computer.
pub fn pick_asset<'a>(app: &AppDef, r: &'a Release) -> Option<&'a Asset> {
    if cfg!(target_os = "macos") {
        let m = app.macos.as_ref()?;
        r.asset("", &m.asset_suffix)
    } else if cfg!(target_os = "windows") {
        let w = app.windows.as_ref()?;
        r.asset(&w.asset_prefix, &w.asset_suffix)
    } else {
        let l = app.linux.as_ref()?;
        r.asset("", &l.asset_suffix)
    }
}

/// Run a command and return its standard output (trimmed), if it succeeded.
fn output(cmd: &mut Command) -> Option<String> {
    let o = cmd.output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

// --------------------------------------------------------------------------
// macOS
// --------------------------------------------------------------------------

/// The folder all apps share (macOS: inside Applications).
pub const FOLDER: &str = "Stagehand";

/// Where an app bundle may be: the shared folder first, then the old places.
#[cfg(target_os = "macos")]
fn mac_locations(bundle: &str) -> Vec<PathBuf> {
    let mut v = vec![Path::new("/Applications").join(FOLDER).join(bundle)];
    if let Some(h) = dirs::home_dir() {
        v.push(h.join("Applications").join(FOLDER).join(bundle));
    }
    v.push(Path::new("/Applications").join(bundle));
    if let Some(h) = dirs::home_dir() {
        v.push(h.join("Applications").join(bundle));
    }
    v
}

/// Windows: the folder an app is installed to, e.g.
/// `C:\Users\me\AppData\Local\Programs\Stagehand\Show Master`.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn windows_app_dir(local_app_data: &Path, app_name: &str) -> PathBuf {
    local_app_data.join("Programs").join(FOLDER).join(app_name)
}

/// Whether `old` is an install folder that can go: it holds the program and
/// the uninstaller and is not (inside) the new one.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn old_install_removable(old: &Path, new: &Path, exe: &str) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
            .replace('/', "\\")
    };
    let (o, n) = (norm(old), norm(new));
    !o.is_empty()
        && o != n
        && !n.starts_with(&format!("{o}\\"))
        && old.join(exe).exists()
        && old.join("unins000.exe").exists()
}

/// `CFBundleShortVersionString` of an app bundle.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn bundle_version(app: &Path) -> Option<String> {
    let text = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    let at = text.find("<key>CFBundleShortVersionString</key>")?;
    let rest = &text[at..];
    let start = rest.find("<string>")? + "<string>".len();
    let end = rest[start..].find("</string>")?;
    Some(rest[start..start + end].trim().to_string())
}

// --------------------------------------------------------------------------
// Windows
// --------------------------------------------------------------------------

/// A value from `reg query` output.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn reg_value(out: &str, name: &str) -> Option<String> {
    out.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        (it.next()? == name).then_some(())?;
        let _kind = it.next()?;
        let rest: Vec<&str> = it.collect();
        (!rest.is_empty()).then(|| rest.join(" "))
    })
}

#[cfg(target_os = "windows")]
fn inno_entry(app_id: &str) -> Option<(Option<String>, PathBuf)> {
    // Per-user installs (the default) and per-machine ones (64- and 32-bit view).
    let tail = format!(r"Microsoft\Windows\CurrentVersion\Uninstall\{app_id}_is1");
    [
        format!(r"HKCU\Software\{tail}"),
        format!(r"HKLM\Software\{tail}"),
        format!(r"HKLM\Software\WOW6432Node\{tail}"),
    ]
    .iter()
    .filter_map(|key| output(Command::new("reg").args(["query", key])))
    .find_map(|out| {
        let loc = reg_value(&out, "InstallLocation")?;
        Some((reg_value(&out, "DisplayVersion"), PathBuf::from(loc)))
    })
}

// --------------------------------------------------------------------------
// Linux
// --------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn linux_managed(app: &AppDef) -> Option<(PathBuf, Option<String>)> {
    let l = app.linux.as_ref()?;
    let dir = crate::paths::linux_apps_dir()?.join(&app.id);
    let bin = dir.join(&l.bin);
    bin.exists().then(|| {
        let v = std::fs::read_to_string(dir.join("VERSION"))
            .ok()
            .map(|s| s.trim().to_string());
        (bin, v)
    })
}

#[cfg(target_os = "linux")]
fn on_path(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(bin))
        .find(|p| p.is_file())
}

// --------------------------------------------------------------------------
// Detect / running / launch
// --------------------------------------------------------------------------

/// The installed copy of `app`, if any.
pub fn installed(app: &AppDef) -> Option<Installed> {
    #[cfg(target_os = "macos")]
    {
        let m = app.macos.as_ref()?;
        mac_locations(&m.app)
            .into_iter()
            .find(|p| p.exists())
            .map(|p| Installed {
                version: bundle_version(&p),
                path: p,
            })
    }
    #[cfg(target_os = "windows")]
    {
        let w = app.windows.as_ref()?;
        let (version, dir) = inno_entry(&w.inno_app_id)?;
        let exe = dir.join(&w.exe);
        exe.exists().then_some(Installed { version, path: exe })
    }
    #[cfg(target_os = "linux")]
    {
        if let Some((bin, version)) = linux_managed(app) {
            return Some(Installed { version, path: bin });
        }
        // Installed by the system (package): ask the program for its version.
        let l = app.linux.as_ref()?;
        let bin = on_path(&l.bin)?;
        let version = output(Command::new(&bin).arg("--version"))
            .and_then(|o| crate::github::find_version(&o));
        Some(Installed { version, path: bin })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = app;
        None
    }
}

/// Whether the app is running right now (then it is not replaced).
pub fn is_running(app: &AppDef, inst: &Installed) -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        let pattern = format!("{}/Contents/MacOS/", inst.path.display());
        Command::new("pgrep")
            .args(["-f", &pattern])
            .output()
            .is_ok_and(|o| o.status.success())
    }
    #[cfg(target_os = "windows")]
    {
        let _ = inst;
        let Some(w) = app.windows.as_ref() else {
            return false;
        };
        output(Command::new("tasklist").args(["/FI", &format!("IMAGENAME eq {}", w.exe), "/NH"]))
            .is_some_and(|o| o.to_lowercase().contains(&w.exe.to_lowercase()))
    }
    #[cfg(target_os = "linux")]
    {
        let name = inst
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .or_else(|| app.linux.as_ref().map(|l| l.bin.clone()))
            .unwrap_or_default();
        Command::new("pgrep")
            .args(["-x", &name])
            .output()
            .is_ok_and(|o| o.status.success())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (app, inst);
        false
    }
}

/// Start the app (detached; Stagehand can be closed afterwards).
pub fn launch(inst: &Installed) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(&inst.path);
        c
    } else {
        let mut c = Command::new(&inst.path);
        if let Some(d) = inst.path.parent() {
            c.current_dir(d);
        }
        c
    };
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start it: {e}"))
}

/// Open a web page in the browser.
pub fn open_url(url: &str) {
    let _ = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).spawn()
    } else if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", "", url]).spawn()
    } else {
        Command::new("xdg-open").arg(url).spawn()
    };
}

// --------------------------------------------------------------------------
// Install
// --------------------------------------------------------------------------

/// Install the downloaded release file of `app`.
pub fn install(app: &AppDef, file: &Path, version: &str) -> Result<(), String> {
    if let Some(inst) = installed(app) {
        if is_running(app, &inst) {
            return Err(format!(
                "{} is running. Quit it first - a show may be going on.",
                app.name
            ));
        }
    }
    install_files(app, file, version)
}

/// [`install`] without the running check (Stagehand updating itself).
pub fn install_files(app: &AppDef, file: &Path, version: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let _ = version;
        let m = app.macos.as_ref().ok_or("not available for macOS")?;
        install_dmg(file, &m.app)
    }
    #[cfg(target_os = "windows")]
    {
        let _ = version;
        let w = app.windows.as_ref().ok_or("not available for Windows")?;
        let before = inno_entry(&w.inno_app_id).map(|(_, dir)| dir);
        let mut cmd = Command::new(file);
        cmd.args([
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CURRENTUSER",
            "/NOCANCEL",
        ]);
        let target = std::env::var_os("LOCALAPPDATA")
            .map(|d| windows_app_dir(Path::new(&d), &app.name));
        if let Some(t) = &target {
            cmd.arg(format!("/DIR={}", t.display()));
        }
        let st = cmd
            .status()
            .map_err(|e| format!("Could not run the installer: {e}"))?;
        if !st.success() {
            return Err(format!("The installer failed ({st})."));
        }
        // Moved from an older place: the old folder is left over.
        if let (Some(old), Some((_, now))) = (before, inno_entry(&w.inno_app_id)) {
            if old_install_removable(&old, &now, &w.exe) {
                let _ = std::fs::remove_dir_all(&old);
            }
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        install_tarball(app, file, version)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (app, file, version);
        Err("not supported on this system".into())
    }
}

#[cfg(target_os = "macos")]
fn install_dmg(dmg: &Path, bundle: &str) -> Result<(), String> {
    let mount = std::env::temp_dir().join(format!("stagehand-{}", std::process::id()));
    std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    let ok = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
        .arg(&mount)
        .arg(dmg)
        .output()
        .map_err(|e| e.to_string())?;
    if !ok.status.success() {
        let _ = std::fs::remove_dir(&mount);
        return Err("Could not open the disk image.".into());
    }
    let result = (|| {
        let src = mount.join(bundle);
        if !src.exists() {
            return Err(format!("{bundle} is not in the disk image."));
        }
        let shared = Path::new("/Applications").join(FOLDER);
        let _ = std::fs::create_dir_all(&shared);
        let dest_dir = if writable(&shared) {
            shared
        } else {
            let d = dirs::home_dir()
                .ok_or("no home folder")?
                .join("Applications")
                .join(FOLDER);
            std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
            d
        };
        let dest = dest_dir.join(bundle);
        let fresh = dest_dir.join(format!(".{bundle}.new"));
        let _ = std::fs::remove_dir_all(&fresh);
        let st = Command::new("ditto")
            .arg(&src)
            .arg(&fresh)
            .status()
            .map_err(|e| e.to_string())?;
        if !st.success() {
            let _ = std::fs::remove_dir_all(&fresh);
            return Err("Copying the app failed.".into());
        }
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::rename(&fresh, &dest).map_err(|e| e.to_string())?;
        let _ = Command::new("xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(&dest)
            .status();
        // Copies in the old places would be found first by Spotlight and the Dock.
        for old in mac_locations(bundle) {
            if old != dest && old.join("Contents/Info.plist").exists() {
                let _ = std::fs::remove_dir_all(&old);
            }
        }
        Ok(())
    })();
    let _ = Command::new("hdiutil")
        .args(["detach", "-quiet"])
        .arg(&mount)
        .status();
    let _ = std::fs::remove_dir(&mount);
    result
}

#[cfg(target_os = "macos")]
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".stagehand-probe-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

#[cfg(target_os = "linux")]
fn install_tarball(app: &AppDef, file: &Path, version: &str) -> Result<(), String> {
    let l = app.linux.as_ref().ok_or("not available for Linux")?;
    let base = crate::paths::linux_apps_dir().ok_or("no home folder")?;
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let tmp = base.join(format!(".{}.new", app.id));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let gz = flate2::read::GzDecoder::new(std::fs::File::open(file).map_err(|e| e.to_string())?);
    tar::Archive::new(gz)
        .unpack(&tmp)
        .map_err(|e| format!("Could not unpack: {e}"))?;
    // The archive holds one folder (name-version-linux-x86_64/); use its content.
    let inner = std::fs::read_dir(&tmp)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.is_dir() && p.join(&l.bin).exists())
        .unwrap_or_else(|| tmp.clone());
    if !inner.join(&l.bin).exists() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("{} is not in the archive.", l.bin));
    }
    std::fs::write(inner.join("VERSION"), version).map_err(|e| e.to_string())?;
    let dest = base.join(&app.id);
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&inner, &dest).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&tmp);
    let bin = dest.join(&l.bin);
    // Menu entry and a command on PATH (~/.local/bin).
    if let Some(data) = dirs::data_dir() {
        let apps = data.join("applications");
        let _ = std::fs::create_dir_all(&apps);
        let icon = ["icon.png", &format!("{}.png", l.bin)]
            .iter()
            .map(|n| dest.join(n))
            .find(|p| p.exists());
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName={}\nComment={}\nExec=\"{}\" %F\n{}Terminal=false\nCategories=AudioVideo;\nX-Stagehand=1\n",
            app.name,
            app.tagline,
            bin.display(),
            icon.map(|i| format!("Icon={}\n", i.display())).unwrap_or_default(),
        );
        let _ = std::fs::write(apps.join(format!("{}.desktop", l.bin)), desktop);
    }
    if let Some(home) = dirs::home_dir() {
        let local_bin = home.join(".local/bin");
        let _ = std::fs::create_dir_all(&local_bin);
        let link = local_bin.join(&l.bin);
        if std::fs::symlink_metadata(&link).is_ok_and(|m| m.file_type().is_symlink()) {
            let _ = std::fs::remove_file(&link);
        }
        if !link.exists() {
            let _ = std::os::unix::fs::symlink(&bin, &link);
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn registry_output() {
        let out = "\r\nHKEY_CURRENT_USER\\Software\\...\\{B7E1}_is1\r\n    DisplayVersion    REG_SZ    0.4.0\r\n    InstallLocation    REG_SZ    C:\\Users\\a b\\AppData\\Local\\Programs\\Show Master\\\r\n";
        assert_eq!(reg_value(out, "DisplayVersion").as_deref(), Some("0.4.0"));
        assert_eq!(
            reg_value(out, "InstallLocation").as_deref(),
            Some("C:\\Users\\a b\\AppData\\Local\\Programs\\Show Master\\")
        );
        assert_eq!(reg_value(out, "Publisher"), None);
    }

    #[test]
    fn windows_folders() {
        let base = Path::new(r"C:\Users\a\AppData\Local");
        let d = windows_app_dir(base, "Show Master");
        assert!(d.ends_with("Stagehand/Show Master") || d.to_string_lossy().ends_with(r"Stagehand\Show Master"));
        // Not removable: missing files, same folder, or the new one inside it.
        let tmp = std::env::temp_dir().join(format!("sh-old-{}", std::process::id()));
        let old = tmp.join("Show Master");
        std::fs::create_dir_all(&old).unwrap();
        let new = tmp.join("Stagehand").join("Show Master");
        assert!(!old_install_removable(&old, &new, "showmaster.exe"));
        std::fs::write(old.join("showmaster.exe"), b"").unwrap();
        std::fs::write(old.join("unins000.exe"), b"").unwrap();
        assert!(old_install_removable(&old, &new, "showmaster.exe"));
        assert!(!old_install_removable(&old, &old, "showmaster.exe"));
        assert!(!old_install_removable(&tmp, &new, "showmaster.exe"));
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn plist_version() {
        let dir = std::env::temp_dir().join(format!("sh-plist-{}", std::process::id()));
        let app = dir.join("X.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        std::fs::write(
            app.join("Contents/Info.plist"),
            "<dict>\n<key>CFBundleName</key><string>X</string>\n    <key>CFBundleShortVersionString</key><string>0.4.0</string>\n</dict>",
        )
        .unwrap();
        assert_eq!(bundle_version(&app).as_deref(), Some("0.4.0"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_tarball_install_and_detect() {
        use crate::catalog::{AppDef, LinuxDef};
        let home = std::env::temp_dir().join(format!("sh-home-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        // Point the data dir and home into the temp folder.
        std::env::set_var("XDG_DATA_HOME", home.join("data"));
        std::env::set_var("HOME", &home);
        // An archive like the Show Master release: one folder with the program.
        let src = home.join("pkg/demo-1.0.0-linux-x86_64");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("demo"), "#!/bin/sh\necho demo 1.0.0\n").unwrap();
        let file = home.join("demo-1.0.0-linux-x86_64.tar.gz");
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(&file).unwrap(),
            flate2::Compression::fast(),
        );
        let mut t = tar::Builder::new(gz);
        t.append_dir_all("demo-1.0.0-linux-x86_64", &src).unwrap();
        t.into_inner().unwrap().finish().unwrap();
        let app = AppDef {
            id: "demo".into(),
            name: "Demo".into(),
            tagline: String::new(),
            color: String::new(),
            releases: "a/b".into(),
            icon_url: None,
            macos: None,
            windows: None,
            linux: Some(LinuxDef {
                asset_suffix: "-linux-x86_64.tar.gz".into(),
                bin: "demo".into(),
            }),
            dev: None,
        };
        install_tarball(&app, &file, "1.0.0").unwrap();
        let inst = installed(&app).unwrap();
        assert_eq!(inst.version.as_deref(), Some("1.0.0"));
        assert!(inst.path.ends_with("stagehand/apps/demo/demo"));
        assert!(home.join("data/applications/demo.desktop").exists());
        assert!(home.join(".local/bin/demo").exists());
        let _ = std::fs::remove_dir_all(home);
    }
}
