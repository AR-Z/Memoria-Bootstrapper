
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use crate::config::{self, Kind};
use crate::ui::{BootstrapState, Phase};

const APP_SETTINGS_XML: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n",
    "<Settings>\r\n",
    "    <ContentFolder>content</ContentFolder>\r\n",
    "    <BaseUrl>https://api.memora.zip</BaseUrl>\r\n",
    "</Settings>\r\n",
);

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) MemoriaBootstrapper/0.1 (+https://memora.zip)";

fn http_client(timeout: Duration) -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(Into::into)
}

fn is_plausible_version(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

fn starts_with_magic(path: &Path, magic: &[u8]) -> bool {
    match fs::File::open(path) {
        Ok(mut f) => {
            let mut buf = vec![0u8; magic.len()];
            f.read_exact(&mut buf).is_ok() && buf == magic
        }
        Err(_) => false,
    }
}

pub fn resolve_target_version(kind: Kind, client: &str) -> Result<String> {
    match fetch_remote_version(kind, client) {
        Ok(v) => Ok(v),
        Err(net_err) => {
            log::warn!("could not reach setup endpoint for {} {client}: {net_err:#}", kind.label());
            if let Ok(file) = config::cached_version_file(kind, client) {
                if let Ok(cached) = fs::read_to_string(file) {
                    let cached = cached.trim().to_string();
                    if !cached.is_empty() {
                        return Ok(cached);
                    }
                }
            }
            Err(net_err.context("no cached version available either"))
        }
    }
}

fn fetch_remote_version(kind: Kind, client: &str) -> Result<String> {
    let http = http_client(Duration::from_secs(15))?;
    let resp = http.get(config::version_url(kind, client)).send()?.error_for_status()?;
    let v = resp.text()?.trim().to_string();
    if !is_plausible_version(&v) {
        return Err(anyhow!("server returned an implausible version string"));
    }
    Ok(v)
}

pub fn is_installed(kind: Kind, client: &str, version: &str) -> bool {
    let exe = match config::target_exe(kind, client) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !exe.is_file() {
        return false;
    }
    match config::cached_version_file(kind, client).and_then(|p| Ok(fs::read_to_string(p)?)) {
        Ok(cached) => cached.trim() == version,
        Err(_) => false,
    }
}

pub fn install_client(
    kind: Kind,
    client: &str,
    version: &str,
    state: &Arc<Mutex<BootstrapState>>,
) -> Result<()> {
    let target = config::install_dir(kind, client)?;
    fs::create_dir_all(&target).with_context(|| format!("creating {}", target.display()))?;

    let zip_path = target.with_extension("zip.partial");
    if zip_path.exists() {
        let _ = fs::remove_file(&zip_path);
    }
    download_with_progress(
        &config::bundle_url(kind, client, version),
        &zip_path,
        state,
        (0.0, 0.7),
    )
    .with_context(|| format!("downloading {} bundle", kind.label()))?;

    if !starts_with_magic(&zip_path, b"PK") {
        let _ = fs::remove_file(&zip_path);
        return Err(anyhow!("downloaded bundle is not a zip archive"));
    }

    let extract_label = format!("Upgrading {}...", kind.label());
    set_status(state, Phase::Configuring, &extract_label);
    extract_zip_with_progress(&zip_path, &target, state, (0.7, 0.95))
        .with_context(|| format!("extracting {} bundle", kind.label()))?;
    let _ = fs::remove_file(&zip_path);

    if kind == Kind::Player {
        let app_settings = target.join("AppSettings.xml");
        if !app_settings.exists() {
            fs::write(&app_settings, APP_SETTINGS_XML)
                .context("writing AppSettings.xml")?;
        }
    }

    if let Ok(file) = config::cached_version_file(kind, client) {
        let _ = fs::create_dir_all(file.parent().unwrap());
        let _ = fs::write(file, version);
    }

    set_progress(state, 0.97);
    Ok(())
}

pub fn install_self() -> Result<PathBuf> {
    let dst = config::launcher_exe_path()?;
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    let src = std::env::current_exe().context("locating current executable")?;
    if same_file(&src, &dst) {
        return Ok(dst);
    }
    if let Err(err) = fs::copy(&src, &dst) {
        if dst.is_file() {
            log::warn!("could not refresh launcher copy ({err}); keeping existing one");
        } else {
            return Err(anyhow!("copying launcher to {}: {err}", dst.display()));
        }
    }
    Ok(dst)
}

pub fn install_launcher_from_server() -> Result<PathBuf> {
    let dst = config::launcher_exe_path()?;
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    let url = config::launcher_url();
    let http = http_client(Duration::from_secs(60 * 5))?;
    match http.get(&url).send().and_then(|r| r.error_for_status()) {
        Ok(mut resp) => {
            let tmp = dst.with_extension("exe.partial");
            if tmp.exists() {
                let _ = fs::remove_file(&tmp);
            }
            let mut file = fs::File::create(&tmp)
                .with_context(|| format!("creating {}", tmp.display()))?;
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = resp.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n])?;
            }
            file.flush()?;
            drop(file);
            if !starts_with_magic(&tmp, b"MZ") {
                let _ = fs::remove_file(&tmp);
                return Err(anyhow!("downloaded launcher is not a valid executable"));
            }
            if let Err(err) = fs::rename(&tmp, &dst) {
                if dst.is_file() {
                    log::warn!(
                        "could not replace launcher at {} ({err}); keeping existing copy",
                        dst.display()
                    );
                    let _ = fs::remove_file(&tmp);
                } else {
                    return Err(anyhow!(
                        "renaming downloaded launcher into place at {}: {err}",
                        dst.display()
                    ));
                }
            }
            Ok(dst)
        }
        Err(err) => {
            log::warn!("could not download launcher from {url}: {err:#}");
            if dst.is_file() {
                Ok(dst)
            } else {
                Err(anyhow!(
                    "no launcher cached locally and download from {url} failed: {err}"
                ))
            }
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}


fn download_with_progress(
    url: &str,
    dest: &Path,
    state: &Arc<Mutex<BootstrapState>>,
    range: (f32, f32),
) -> Result<()> {
    let http = http_client(Duration::from_secs(60 * 10))?;
    let mut resp = http.get(url).send()?.error_for_status()?;
    let total = resp.content_length();

    let mut file = fs::File::create(dest)
        .with_context(|| format!("creating {}", dest.display()))?;
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;
        if let Some(total) = total {
            if total > 0 {
                let frac = (downloaded as f32) / (total as f32);
                set_progress(state, lerp(range.0, range.1, frac.clamp(0.0, 1.0)));
            }
        }
    }
    file.flush()?;
    set_progress(state, range.1);
    Ok(())
}

fn extract_zip_with_progress(
    zip_path: &Path,
    target: &Path,
    state: &Arc<Mutex<BootstrapState>>,
    range: (f32, f32),
) -> Result<()> {
    let file = fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let total = zip.len() as f32;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let rel = match entry.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };
        let out = target.join(&rel);

        if entry.is_dir() {
            fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut writer = fs::File::create(&out)
                .with_context(|| format!("creating {}", out.display()))?;
            io::copy(&mut entry, &mut writer)?;
        }

        let frac = ((i + 1) as f32) / total;
        set_progress(state, lerp(range.0, range.1, frac.clamp(0.0, 1.0)));
    }

    Ok(())
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn set_status(state: &Arc<Mutex<BootstrapState>>, phase: Phase, status: &str) {
    let mut s = state.lock().unwrap();
    s.phase = phase;
    s.status = status.to_string();
}

fn set_progress(state: &Arc<Mutex<BootstrapState>>, p: f32) {
    let mut s = state.lock().unwrap();
    s.progress = Some(p.clamp(0.0, 1.0));
}


#[cfg(windows)]
pub fn register_url_schemes(launcher: &Path) -> Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let launcher_str = launcher.to_string_lossy().to_string();

    let register = [config::URL_SCHEMES_PLAYER[0], config::URL_SCHEMES_STUDIO[0]];
    for scheme in register {
        let base = format!("Software\\Classes\\{scheme}");
        let (key, _) = hkcu.create_subkey(&base)?;
        key.set_value("", &format!("URL:{} Protocol", config::APP_NAME))?;
        key.set_value("URL Protocol", &"")?;

        let (icon_key, _) = hkcu.create_subkey(format!("{base}\\DefaultIcon"))?;
        icon_key.set_value("", &format!("\"{launcher_str}\",0"))?;

        let (cmd_key, _) = hkcu.create_subkey(format!("{base}\\shell\\open\\command"))?;
        cmd_key.set_value("", &format!("\"{launcher_str}\" \"%1\""))?;
    }

    for scheme in config::URL_SCHEMES_PLAYER
        .iter()
        .chain(config::URL_SCHEMES_STUDIO.iter())
    {
        if *scheme == register[0] || *scheme == register[1] {
            continue;
        }
        let _ = hkcu.delete_subkey_all(format!("Software\\Classes\\{scheme}"));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn register_url_schemes(_launcher: &Path) -> Result<()> {
    Ok(())
}


#[cfg(windows)]
pub fn register_file_associations(launcher: &Path) -> Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let launcher_str = launcher.to_string_lossy().to_string();

    let prog_id = "Memoria.StudioFile";
    let (cls, _) = hkcu.create_subkey(format!("Software\\Classes\\{prog_id}"))?;
    cls.set_value("", &"Memoria Studio File")?;
    cls.set_value("FriendlyTypeName", &"Memoria Studio File")?;

    let (icon, _) = hkcu.create_subkey(format!("Software\\Classes\\{prog_id}\\DefaultIcon"))?;
    icon.set_value("", &format!("\"{launcher_str}\",0"))?;

    let (cmd, _) = hkcu.create_subkey(format!(
        "Software\\Classes\\{prog_id}\\shell\\open\\command"
    ))?;
    cmd.set_value("", &format!("\"{launcher_str}\" \"%1\""))?;

    for ext in config::STUDIO_FILE_EXTS {
        let (ext_key, _) = hkcu.create_subkey(format!("Software\\Classes\\.{ext}"))?;
        ext_key.set_value("", &prog_id)?;
        let (open_with, _) =
            hkcu.create_subkey(format!("Software\\Classes\\.{ext}\\OpenWithProgids"))?;
        open_with.set_value(prog_id, &"")?;
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn register_file_associations(_launcher: &Path) -> Result<()> {
    Ok(())
}


pub fn self_update_if_needed(args: &[String]) -> bool {
    if std::env::var("MEMORIA_NO_SELFUPDATE").is_ok() {
        return false;
    }
    let http = match http_client(Duration::from_secs(15)) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let live = match http
        .get(config::launcher_version_url())
        .send()
        .and_then(|r| r.error_for_status())
    {
        Ok(r) => r.text().unwrap_or_default().trim().to_string(),
        Err(_) => return false,
    };
    if !is_plausible_version(&live) || live == config::LAUNCHER_VERSION {
        return false;
    }
    log::warn!("launcher {} -> {}; self-updating", config::LAUNCHER_VERSION, live);
    let installed = match config::launcher_exe_path() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let root = match config::install_root() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let _ = fs::create_dir_all(&root);
    let staged = root.join("bootstrapper.update.exe");
    let _ = fs::remove_file(&staged);
    let download = (|| -> Result<()> {
        let mut resp = http.get(config::launcher_url()).send()?.error_for_status()?;
        let mut file = fs::File::create(&staged)?;
        let mut buf = [0u8; 65536];
        loop {
            let n = resp.read(&mut buf)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
        }
        Ok(())
    })();
    if download.is_err() {
        let _ = fs::remove_file(&staged);
        return false;
    }
    if !starts_with_magic(&staged, b"MZ") {
        let _ = fs::remove_file(&staged);
        return false;
    }
    let old = root.join("bootstrapper.old.exe");
    let _ = fs::remove_file(&old);
    if installed.exists() {
        let _ = fs::rename(&installed, &old);
    }
    if fs::rename(&staged, &installed).is_err() && fs::copy(&staged, &installed).is_err() {
        return false;
    }
    let mut cmd = std::process::Command::new(&installed);
    cmd.args(args);
    cmd.env("MEMORIA_NO_SELFUPDATE", "1");
    if let Some(parent) = installed.parent() {
        cmd.current_dir(parent);
    }
    cmd.spawn().is_ok()
}

pub fn enforce_global_version(state: &Arc<Mutex<BootstrapState>>) -> Result<()> {
    let http = http_client(Duration::from_secs(15))?;
    let live = http
        .get(config::global_version_url())
        .send()?
        .error_for_status()?
        .text()?
        .trim()
        .to_string();
    if !is_plausible_version(&live) {
        return Err(anyhow!("server returned an implausible global version"));
    }

    let cache_path = config::global_version_file()?;
    let cached = fs::read_to_string(&cache_path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    if cached == live {
        return Ok(());
    }

    if !cached.is_empty() {
        log::warn!(
            "global version changed ({} -> {}); wiping all installs",
            cached,
            live
        );
        set_status(state, Phase::Configuring, "Updating Memoria to a new version...");
        wipe_all_installs()?;
    }

    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&cache_path, &live)
        .with_context(|| format!("writing {}", cache_path.display()))?;
    Ok(())
}

fn wipe_all_installs() -> Result<()> {
    for kind in &[Kind::Player, Kind::Studio] {
        let root = config::kind_root(*kind)?;
        if root.exists() {
            log::info!("removing {}", root.display());
            if let Err(err) = fs::remove_dir_all(&root) {
                log::warn!("could not fully remove {}: {err}", root.display());
            }
        }
    }
    if let Ok(state) = config::state_dir() {
        if state.is_dir() {
            for entry in fs::read_dir(&state)?.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("LATEST-") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
    Ok(())
}
