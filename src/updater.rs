//! Release versioning, update feed checks, SHA-256 verification, and atomic
//! `.app` bundle replacement for Knack on macOS.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_FEED_URL: &str = "https://knack.joncallahan.com/releases/latest.json";
pub const RELEASES_URL: &str = "https://github.com/jondcallahan/knack/releases";

pub fn feed_url() -> String {
    match std::env::var("KNACK_UPDATE_URL") {
        Ok(url) if !url.trim().is_empty() => url,
        _ => DEFAULT_FEED_URL.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub version: String,
    pub tag: String,
    #[serde(default)]
    pub published_at: String,
    #[serde(default)]
    pub notes: String,
    pub release_url: String,
    #[serde(default)]
    pub dmg_url: String,
    pub zip_url: String,
    #[serde(default)]
    pub zip_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheck {
    UpToDate { current: String },
    Available(ReleaseManifest),
}

pub fn parse_semver(version: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let trimmed = version.trim().strip_prefix('v').unwrap_or(version.trim());
    if trimmed.is_empty() {
        return None;
    }
    let (core, pre) = match trimmed.split_once('-') {
        Some((core, pre)) if !pre.is_empty() => (core, Some(pre.to_string())),
        Some((core, _)) => (core, None),
        None => (trimmed, None),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch, pre))
}

pub fn is_newer(remote: &str, current: &str) -> bool {
    let Some((r_maj, r_min, r_pat, r_pre)) = parse_semver(remote) else {
        return false;
    };
    let Some((c_maj, c_min, c_pat, c_pre)) = parse_semver(current) else {
        return false;
    };
    match (r_maj, r_min, r_pat).cmp(&(c_maj, c_min, c_pat)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match (r_pre, c_pre) {
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (Some(r), Some(c)) => r > c,
            (None, None) => false,
        },
    }
}

pub fn parse_manifest(raw: &str) -> Result<ReleaseManifest> {
    serde_json::from_str::<ReleaseManifest>(raw).context("parse release manifest JSON")
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let output = Command::new("/usr/bin/shasum")
        .arg("-a")
        .arg("256")
        .arg(path)
        .output()
        .context("run shasum")?;
    if !output.status.success() {
        bail!(
            "shasum failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let hex = stdout
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow!("unexpected shasum output"))?
        .to_ascii_lowercase();
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("invalid sha256 digest: {hex}");
    }
    Ok(hex)
}

pub fn current_app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let macos_dir = exe.parent()?;
    if macos_dir.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let app_dir = contents_dir.parent()?;
    (app_dir.extension()?.to_str()? == "app").then(|| app_dir.to_path_buf())
}

pub fn swap_app_bundle(new_app: &Path, dest_app: &Path) -> Result<()> {
    if !new_app.is_dir() {
        bail!("replacement bundle missing: {}", new_app.display());
    }
    let parent = dest_app
        .parent()
        .ok_or_else(|| anyhow!("invalid destination bundle path"))?;
    let dest_name = dest_app
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("invalid destination bundle name"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let backup = parent.join(format!(".{dest_name}.old-{stamp}"));
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }

    let had_existing = dest_app.exists();
    if had_existing {
        fs::rename(dest_app, &backup).with_context(|| {
            format!("back up existing bundle {} -> {}", dest_app.display(), backup.display())
        })?;
    }

    if let Err(err) = fs::rename(new_app, dest_app) {
        if had_existing {
            let _ = fs::rename(&backup, dest_app);
        }
        return Err(err).with_context(|| {
            format!("move new bundle {} -> {}", new_app.display(), dest_app.display())
        });
    }

    if had_existing {
        let _ = fs::remove_dir_all(&backup);
    }
    Ok(())
}

pub fn check_for_updates(url: &str) -> Result<UpdateCheck> {
    let raw = if let Some(path) = url.strip_prefix("file://") {
        fs::read_to_string(path).with_context(|| format!("read feed file {path}"))?
    } else {
        let output = Command::new("/usr/bin/curl")
            .args(["-fsSL", "--max-time", "10", url])
            .output()
            .context("run curl to check updates")?;
        if !output.status.success() {
            bail!(
                "Update check failed ({})",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        String::from_utf8(output.stdout).context("decode release manifest as UTF-8")?
    };

    let manifest = parse_manifest(&raw)?;
    if is_newer(&manifest.version, CURRENT_VERSION) {
        Ok(UpdateCheck::Available(manifest))
    } else {
        Ok(UpdateCheck::UpToDate {
            current: CURRENT_VERSION.to_string(),
        })
    }
}

pub fn download_and_install(manifest: &ReleaseManifest, dest_app: &Path) -> Result<()> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let work_dir = std::env::temp_dir().join(format!("knack-update-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&work_dir).context("create update staging dir")?;
    let result = install_in_work_dir(manifest, dest_app, &work_dir);
    let _ = fs::remove_dir_all(&work_dir);
    result
}

fn install_in_work_dir(
    manifest: &ReleaseManifest,
    dest_app: &Path,
    work_dir: &Path,
) -> Result<()> {
    let zip_path = work_dir.join("Knack.zip");
    if let Some(src_path) = manifest.zip_url.strip_prefix("file://") {
        fs::copy(src_path, &zip_path).with_context(|| format!("copy {src_path}"))?;
    } else {
        let output = Command::new("/usr/bin/curl")
            .args(["-fsSL", "--max-time", "120", "-o"])
            .arg(&zip_path)
            .arg(&manifest.zip_url)
            .output()
            .context("download update archive")?;
        if !output.status.success() {
            bail!(
                "Download failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
    }

    let expected_sha = manifest.zip_sha256.trim().to_ascii_lowercase();
    if !expected_sha.is_empty() {
        let actual_sha = sha256_file(&zip_path)?;
        if actual_sha != expected_sha {
            bail!("Checksum mismatch (expected {expected_sha}, got {actual_sha})");
        }
    }

    let extract_dir = work_dir.join("extracted");
    fs::create_dir_all(&extract_dir).context("create extraction directory")?;
    let extract = Command::new("/usr/bin/ditto")
        .arg("-xk")
        .arg(&zip_path)
        .arg(&extract_dir)
        .output()
        .context("run ditto to extract update")?;
    if !extract.status.success() {
        bail!(
            "Failed to unpack update: {}",
            String::from_utf8_lossy(&extract.stderr).trim()
        );
    }

    let candidate = extract_dir.join("Knack.app");
    if !candidate.join("Contents/MacOS/knack").is_file() {
        bail!("Downloaded archive does not contain a valid Knack.app");
    }

    // Move into a sibling directory on the same volume as `dest_app` so `fs::rename` is atomic.
    let parent = dest_app
        .parent()
        .ok_or_else(|| anyhow!("invalid install directory"))?;
    let staged = parent.join(format!(".Knack.app.staged-{}", std::process::id()));
    if staged.exists() {
        let _ = fs::remove_dir_all(&staged);
    }
    let copy = Command::new("/usr/bin/ditto")
        .arg(&candidate)
        .arg(&staged)
        .output()
        .context("stage extracted bundle")?;
    if !copy.status.success() {
        let _ = fs::remove_dir_all(&staged);
        bail!(
            "Failed to stage update: {}",
            String::from_utf8_lossy(&copy.stderr).trim()
        );
    }

    if let Err(err) = swap_app_bundle(&staged, dest_app) {
        let _ = fs::remove_dir_all(&staged);
        return Err(err);
    }

    let _ = Command::new("/usr/bin/xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(dest_app)
        .output();

    Ok(())
}

pub fn schedule_relaunch(dest_app: &Path) -> Result<()> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg("sleep 0.4; open -n \"$1\" >/dev/null 2>&1 &")
        .arg("_")
        .arg(dest_app)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("schedule relaunch")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{is_newer, parse_manifest, parse_semver, sha256_file, swap_app_bundle};

    #[test]
    fn semver_comparison_handles_major_minor_patch_and_prerelease() {
        assert_eq!(parse_semver("0.1.0"), Some((0, 1, 0, None)));
        assert_eq!(
            parse_semver("v1.2.3-beta.1"),
            Some((1, 2, 3, Some("beta.1".into())))
        );
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(is_newer("0.2.0", "0.2.0-rc.1"));
        assert!(!is_newer("0.2.0-rc.1", "0.2.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
    }

    #[test]
    fn parses_manifest_and_detects_newer_version() {
        let raw = r#"{
          "version": "0.2.0",
          "tag": "v0.2.0",
          "published_at": "2026-11-24T00:00:00Z",
          "notes": "Added updater.",
          "release_url": "https://github.com/jondcallahan/knack/releases/tag/v0.2.0",
          "dmg_url": "https://github.com/jondcallahan/knack/releases/download/v0.2.0/Knack-0.2.0-arm64.dmg",
          "zip_url": "https://github.com/jondcallahan/knack/releases/download/v0.2.0/Knack-0.2.0-arm64.zip",
          "zip_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        }"#;
        let m = parse_manifest(raw).unwrap();
        assert_eq!(m.version, "0.2.0");
        assert_eq!(m.tag, "v0.2.0");
        assert!(is_newer(&m.version, "0.1.0"));
    }

    #[test]
    fn sha256_and_bundle_swap_work_and_rollback_cleanly() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("hello.txt");
        fs::write(&file, "knack").unwrap();
        let digest = sha256_file(&file).unwrap();
        assert_eq!(digest.len(), 64);

        let dest = temp.path().join("Knack.app");
        let new_app = temp.path().join("NewKnack.app");
        fs::create_dir_all(dest.join("Contents/MacOS")).unwrap();
        fs::write(dest.join("Contents/MacOS/knack"), "old").unwrap();
        fs::create_dir_all(new_app.join("Contents/MacOS")).unwrap();
        fs::write(new_app.join("Contents/MacOS/knack"), "new").unwrap();

        swap_app_bundle(&new_app, &dest).unwrap();
        assert_eq!(
            fs::read_to_string(dest.join("Contents/MacOS/knack")).unwrap(),
            "new"
        );
        assert!(!new_app.exists());

        // Attempting to swap in a non-existent bundle fails without harming `dest`.
        let missing = temp.path().join("Missing.app");
        assert!(swap_app_bundle(&missing, &dest).is_err());
        assert_eq!(
            fs::read_to_string(dest.join("Contents/MacOS/knack")).unwrap(),
            "new"
        );
    }
}
