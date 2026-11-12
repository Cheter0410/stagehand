//! Releases on GitHub: the newest version of an app, its download for this
//! computer, and downloading it (checked against the SHA-256 GitHub lists).

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const AGENT: &str = concat!("Stagehand/", env!("CARGO_PKG_VERSION"));

/// HTTP client: the system's certificates and its proxy setting
/// (`HTTPS_PROXY`), as networks in venues and companies often need.
pub fn http() -> ureq::Agent {
    static AGENT_CELL: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT_CELL
        .get_or_init(|| ureq::AgentBuilder::new().try_proxy_from_env(true).build())
        .clone()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Hex SHA-256, when GitHub lists one.
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// The asset whose name starts with `prefix` and ends with `suffix`.
    pub fn asset(&self, prefix: &str, suffix: &str) -> Option<&Asset> {
        self.assets
            .iter()
            .find(|a| a.name.starts_with(prefix) && a.name.ends_with(suffix))
    }
}

/// GET a text document; `Ok(None)` for 404.
pub fn get_text(url: &str) -> Result<Option<String>, String> {
    match http()
        .get(url)
        .set("User-Agent", AGENT)
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .call()
    {
        Ok(r) => r.into_string().map(Some).map_err(|e| e.to_string()),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(ureq::Error::Status(403 | 429, _)) => {
            Err("GitHub asks to wait a little (too many checks) - try again later.".into())
        }
        Err(ureq::Error::Status(code, _)) => Err(format!("GitHub answered with error {code}.")),
        Err(_) => Err("No internet connection.".into()),
    }
}

/// The newest release of `repo` (`owner/name`); `Ok(None)` if there is none yet.
pub fn latest(repo: &str) -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let Some(text) = get_text(&url)? else {
        return Ok(None);
    };
    let json: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(parse_release(&json))
}

pub fn parse_release(json: &Value) -> Option<Release> {
    let tag = json.get("tag_name")?.as_str()?;
    let assets = json
        .get("assets")
        .and_then(Value::as_array)
        .map(|v| {
            v.iter()
                .filter_map(|a| {
                    Some(Asset {
                        name: a.get("name")?.as_str()?.to_string(),
                        url: a.get("browser_download_url")?.as_str()?.to_string(),
                        size: a.get("size").and_then(Value::as_u64).unwrap_or(0),
                        sha256: a
                            .get("digest")
                            .and_then(Value::as_str)
                            .and_then(|d| d.strip_prefix("sha256:"))
                            .map(str::to_lowercase),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Release {
        version: tag.trim_start_matches('v').to_string(),
        notes: json
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        page: json
            .get("html_url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        assets,
    })
}

fn parts(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map_while(|p| p.parse().ok())
        .collect()
}

/// `a` is a newer version than `b` (numeric, `0.10.0 > 0.9.9`).
pub fn is_newer(a: &str, b: &str) -> bool {
    let (mut x, mut y) = (parts(a), parts(b));
    let n = x.len().max(y.len());
    x.resize(n, 0);
    y.resize(n, 0);
    x > y
}

/// First `x.y.z` in a text (e.g. the output of `prog --version`).
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn find_version(text: &str) -> Option<String> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|w| w.split('.').filter(|p| !p.is_empty()).count() >= 2 && !w.starts_with('.'))
        .map(|w| w.trim_end_matches('.').to_string())
}

/// Download `a` into `dir`, reporting (done, total) bytes; checks the SHA-256.
pub fn download(a: &Asset, dir: &Path, progress: &dyn Fn(u64, u64)) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(&a.name);
    let part = dir.join(format!("{}.part", a.name));
    let resp = http()
        .get(&a.url)
        .set("User-Agent", AGENT)
        .timeout(Duration::from_secs(60 * 30))
        .call()
        .map_err(|_| {
            "The download could not be started. Check the internet connection.".to_string()
        })?;
    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(a.size);
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|_| "The download was interrupted. Try again.".to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buf[..n]);
        done += n as u64;
        progress(done, total);
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if let Some(want) = &a.sha256 {
        let got: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if &got != want {
            let _ = std::fs::remove_file(&part);
            return Err("The download is damaged (checksum mismatch). Try again.".into());
        }
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("v0.4.1", "0.4.0"));
        assert!(!is_newer("0.4.0", "0.4"));
        assert!(!is_newer("0.3.9", "0.4.0"));
    }

    #[test]
    fn version_in_program_output() {
        assert_eq!(find_version("showmaster 0.4.0\n").as_deref(), Some("0.4.0"));
        assert_eq!(find_version("Beamweave v1.2").as_deref(), Some("1.2"));
        assert_eq!(find_version("no version"), None);
    }

    #[test]
    fn release_json() {
        let j: Value = serde_json::from_str(
            r#"{"tag_name":"v0.4.0","body":"- new\n","html_url":"https://x/r",
            "assets":[{"name":"ShowMaster-0.4.0.dmg","browser_download_url":"https://x/a.dmg","size":5,"digest":"sha256:ABCD"},
                      {"name":"ShowMaster-Setup-0.4.0.exe","browser_download_url":"https://x/s.exe","size":7}]}"#,
        )
        .unwrap();
        let r = parse_release(&j).unwrap();
        assert_eq!(r.version, "0.4.0");
        assert_eq!(r.notes, "- new");
        assert_eq!(r.asset("", ".dmg").unwrap().sha256.as_deref(), Some("abcd"));
        assert_eq!(r.asset("ShowMaster-Setup-", ".exe").unwrap().size, 7);
        assert!(r.asset("Beamweave-Setup-", ".exe").is_none());
    }
}
