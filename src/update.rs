//! Update checks against the project's published GitHub releases, and in-place installs: the
//! release file for this platform is downloaded, checked against `SHA256SUMS.txt`, and swapped in
//! by a small helper that waits for Strata to quit and then starts the new version.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::Digest;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub const REPO_URL: &str = "https://github.com/fikrifirmanf/strata";
const LATEST_API: &str = "https://api.github.com/repos/fikrifirmanf/strata/releases/latest";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub notes: String,
    pub published: Option<chrono::DateTime<chrono::Utc>>,
    /// This platform's release file, when the release has one.
    pub asset: Option<Asset>,
    pub sums_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    published_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// Suffix of the release file this build installs from, matching the `scripts/package-*` names.
const ASSET_SUFFIX: &str = if cfg!(target_os = "macos") {
    "-macos-universal.dmg"
} else if cfg!(target_os = "windows") {
    "-windows-x86_64-setup.exe"
} else {
    "-linux-x86_64.tar.gz"
};

/// `v1.2.3`, `1.2` and `1.2.3-beta.1` parse; pre-release suffixes are ignored for ordering.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let core = s.trim().trim_start_matches(['v', 'V']).split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>());
    let major = parts.next()?.ok()?;
    let minor = parts.next().unwrap_or(Ok(0)).ok()?;
    let patch = parts.next().unwrap_or(Ok(0)).ok()?;
    Some((major, minor, patch))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse_version(candidate), parse_version(current)), (Some(c), Some(cur)) if c > cur)
}

/// The latest release when it is newer than this build, `None` when this build is current.
pub async fn check() -> Result<Option<Release>> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build()?;
    let resp = client
        .get(LATEST_API)
        .header("User-Agent", format!("Strata/{CURRENT}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Can't reach GitHub")?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        bail!("No releases have been published yet");
    }
    if !resp.status().is_success() {
        bail!("GitHub answered {}", resp.status());
    }
    let latest: GithubRelease = resp.json().await.context("Unexpected answer from GitHub")?;
    if !is_newer(&latest.tag_name, CURRENT) {
        return Ok(None);
    }
    Ok(Some(Release {
        version: latest.tag_name.trim_start_matches(['v', 'V']).to_string(),
        url: latest.html_url,
        notes: latest.body.unwrap_or_default().trim().to_string(),
        published: latest.published_at,
        sums_url: latest.assets.iter().find(|a| a.name == "SHA256SUMS.txt").map(|a| a.url.clone()),
        asset: latest.assets.into_iter().find(|a| a.name.ends_with(ASSET_SUFFIX)),
    }))
}

/// Where this copy of Strata is installed, when it can replace itself: the `.app` bundle on macOS,
/// an Inno Setup install on Windows (the portable zip can't), or the binary itself on Linux.
pub fn install_target() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    if cfg!(debug_assertions) {
        return None;
    }
    if cfg!(target_os = "macos") {
        exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
    } else if cfg!(target_os = "windows") {
        exe.parent().filter(|dir| dir.join("unins000.exe").exists()).map(|_| exe.clone())
    } else {
        Some(exe)
    }
}

#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
}

/// Downloads and verifies the release file, stages it, and starts the helper that installs it once
/// this process exits. The caller should quit right after this returns `Ok`.
pub async fn install(release: Release, progress: Arc<Progress>, repaint: impl Fn() + Send + 'static) -> Result<()> {
    let target = install_target().context("This copy of Strata can't update itself")?;
    let asset = release.asset.context("The release has no file for this platform")?;
    let client = reqwest::Client::builder().connect_timeout(std::time::Duration::from_secs(15)).build()?;
    let get = |url: &str| client.get(url).header("User-Agent", format!("Strata/{CURRENT}"));

    let dir = std::env::temp_dir().join(format!("strata-update-{}", release.version));
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir).await?;
    let file = dir.join(&asset.name);

    let mut resp = get(&asset.url).send().await.context("Can't reach GitHub")?.error_for_status()?;
    progress.total.store(resp.content_length().unwrap_or(asset.size), Ordering::Relaxed);
    let mut hasher = sha2::Sha256::new();
    let mut out = tokio::fs::File::create(&file).await?;
    while let Some(chunk) = resp.chunk().await.context("Download interrupted")? {
        hasher.update(&chunk);
        tokio::io::AsyncWriteExt::write_all(&mut out, &chunk).await?;
        progress.done.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        repaint();
    }
    tokio::io::AsyncWriteExt::flush(&mut out).await?;
    drop(out);

    let sums_url = release.sums_url.context("The release has no SHA256SUMS.txt")?;
    let sums = get(&sums_url).send().await?.error_for_status()?.text().await?;
    let expected = expected_hash(&sums, &asset.name).context("SHA256SUMS.txt has no entry for the download")?;
    if hex::encode(hasher.finalize()) != expected {
        bail!("The download doesn't match its checksum");
    }

    let file2 = file.clone();
    tokio::task::spawn_blocking(move || stage_and_spawn(&file2, &dir, &target)).await??;
    Ok(())
}

/// The hash listed for `file` in `shasum -a 256` output (`<hash>  <file>`, or `*<file>` in binary mode).
fn expected_hash(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|l| l.split_once(char::is_whitespace).filter(|(_, f)| f.trim().trim_start_matches('*') == file).map(|(h, _)| h.to_lowercase()))
}

#[cfg(not(target_os = "windows"))]
fn run(cmd: &mut std::process::Command) -> Result<()> {
    let out = cmd.output().with_context(|| format!("Couldn't run {:?}", cmd.get_program()))?;
    if !out.status.success() {
        bail!("{:?} failed: {}", cmd.get_program(), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn stage_and_spawn(dmg: &Path, dir: &Path, app: &Path) -> Result<()> {
    use std::process::Command;
    let mount = dir.join("mount");
    std::fs::create_dir_all(&mount)?;
    run(Command::new("hdiutil").arg("attach").arg(dmg).args(["-nobrowse", "-readonly", "-noautoopen", "-quiet", "-mountpoint"]).arg(&mount))?;
    // Staged next to the old bundle so the final swap is a rename on the same volume.
    let staged = app.with_file_name(".Strata-update.app");
    let _ = std::fs::remove_dir_all(&staged);
    let copied = run(Command::new("ditto").arg(mount.join("Strata.app")).arg(&staged));
    let _ = Command::new("hdiutil").arg("detach").arg(&mount).arg("-quiet").status();
    copied.with_context(|| format!("Can't write to {}", app.parent().unwrap_or(app).display()))?;
    let _ = Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(&staged).status();
    let script = r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done
mv "$3" "$3.old" && mv "$2" "$3" && rm -rf "$3.old" || mv "$3.old" "$3"
open "$3""#;
    Command::new("/bin/sh").args(["-c", script, "sh"]).arg(std::process::id().to_string()).arg(&staged).arg(app).spawn()?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn stage_and_spawn(setup: &Path, _dir: &Path, _exe: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // The installer reuses the previous install folder and mode, and relaunches Strata when silent.
    let script = format!(
        "Wait-Process -Id {} -ErrorAction SilentlyContinue; Start-Process -FilePath '{}' -ArgumentList '/SILENT','/SUPPRESSMSGBOXES','/NORESTART'",
        std::process::id(),
        setup.display().to_string().replace('\'', "''")
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn stage_and_spawn(tarball: &Path, dir: &Path, exe: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    run(Command::new("tar").arg("-xzf").arg(tarball).arg("-C").arg(dir))?;
    let new_bin = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path().join("strata"))
        .find(|p| p.is_file())
        .context("The download has no strata binary")?;
    // Linux lets a running binary be replaced by renaming over it.
    let staged = exe.with_file_name(".strata-update");
    std::fs::copy(&new_bin, &staged).with_context(|| format!("Can't write to {}", exe.parent().unwrap_or(exe).display()))?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&staged, exe)?;
    let script = r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done
exec "$2""#;
    Command::new("/bin/sh").args(["-c", script, "sh"]).arg(std::process::id().to_string()).arg(exe).spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically_not_as_text() {
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0-beta.2", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("nightly", "0.1.0"));
        assert_eq!(parse_version("V2.3.4+build.7"), Some((2, 3, 4)));
    }

    #[test]
    fn checksum_lookup_matches_the_exact_file_name() {
        let sums = "AA11  Strata-0.2.0-macos-universal.dmg\nbb22 *strata-0.2.0-windows-x86_64.zip\ncc33  Strata-0.2.0-windows-x86_64-setup.exe\n";
        assert_eq!(expected_hash(sums, "Strata-0.2.0-macos-universal.dmg").as_deref(), Some("aa11"));
        assert_eq!(expected_hash(sums, "strata-0.2.0-windows-x86_64.zip").as_deref(), Some("bb22"));
        assert_eq!(expected_hash(sums, "Strata-0.2.0-windows-x86_64-setup.exe").as_deref(), Some("cc33"));
        assert_eq!(expected_hash(sums, "Strata-0.2.0-macos.dmg"), None);
    }
}
