
use std::path::Path;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use percent_encoding::percent_decode_str;

use crate::config::{self, Kind};
use crate::installer;
use crate::ui::{BootstrapState, Phase};

pub struct LaunchInput {
    pub play_url: Option<String>,
    pub local_file: Option<String>,
    pub client: String,
    pub kind: Kind,
    pub install_only: bool,
    pub launch_when_idle: bool,
}

fn looks_like_studio_file(arg: &str) -> bool {
    if let Some(ext) = std::path::Path::new(arg)
        .extension()
        .and_then(|e| e.to_str())
    {
        let lc = ext.to_ascii_lowercase();
        config::STUDIO_FILE_EXTS.iter().any(|e| *e == lc)
    } else {
        false
    }
}

pub fn parse_argv(args: &[String]) -> LaunchInput {
    let mut play_url: Option<String> = None;
    let mut local_file: Option<String> = None;
    let mut client: Option<String> = None;
    let mut kind: Option<Kind> = None;
    let mut install_only = false;
    let mut launch_when_idle = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--play-url" => {
                if let Some(v) = iter.next() {
                    play_url = Some(v.clone());
                }
            }
            "--client" => {
                if let Some(v) = iter.next() {
                    client = Some(v.clone());
                }
            }
            "--install" | "--installer" => install_only = true,
            "--launch" => launch_when_idle = true,
            "--studio" => kind = Some(Kind::Studio),
            "--player" => kind = Some(Kind::Player),
            "--open" => {
                if let Some(v) = iter.next() {
                    local_file = Some(v.clone());
                    if kind.is_none() {
                        kind = Some(Kind::Studio);
                    }
                }
            }
            other => {
                if let Some((scheme, _)) = other.split_once(':') {
                    if let Some(k) = Kind::from_scheme(scheme) {
                        play_url = Some(other.to_string());
                        if kind.is_none() {
                            kind = Some(k);
                        }
                        continue;
                    }
                }
                if looks_like_studio_file(other) {
                    local_file = Some(other.to_string());
                    if kind.is_none() {
                        kind = Some(Kind::Studio);
                    }
                }
            }
        }
    }
    if client.is_none() {
        if let Some(url) = play_url.as_deref() {
            if let Ok(parsed) = parse_play_uri(url) {
                if let Some(year) = parsed.clientyear {
                    client = Some(year);
                }
            }
        }
    }
    LaunchInput {
        play_url,
        local_file,
        client: client.unwrap_or_else(|| config::DEFAULT_CLIENT.to_string()),
        kind: kind.unwrap_or(Kind::Player),
        install_only,
        launch_when_idle,
    }
}

#[derive(Debug, Default)]
pub struct PlayParams {
    pub launchmode: String,
    pub gameinfo: Option<String>,
    pub placelauncherurl: Option<String>,
    pub launchtime: Option<String>,
    pub clientversion: Option<String>,
    pub clientyear: Option<String>,
    pub launchexp: Option<String>,
    pub browsertrackerid: Option<String>,

    pub task: Option<String>,
    pub place_id: Option<String>,
    pub universe_id: Option<String>,
    pub user_id: Option<String>,
    pub script: Option<String>,
}

pub fn parse_play_uri(uri: &str) -> Result<PlayParams> {
    let after_scheme = uri
        .splitn(2, ':')
        .nth(1)
        .ok_or_else(|| anyhow!("missing scheme separator in URI"))?;

    let mut p = PlayParams::default();
    for token in after_scheme.split('+') {
        if token.is_empty() {
            continue;
        }
        let (key, value) = match token.split_once(':') {
            Some(kv) => kv,
            None => continue,
        };
        let value = percent_decode_str(value).decode_utf8_lossy().to_string();
        match key {
            "1" | "launchmode" => p.launchmode = value,
            "gameinfo" => p.gameinfo = Some(value),
            "placelauncherurl" => p.placelauncherurl = Some(value),
            "launchtime" => p.launchtime = Some(value),
            "clientversion" => p.clientversion = Some(value),
            "clientyear" => p.clientyear = Some(value),
            "launchexp" => p.launchexp = Some(value),
            "browsertrackerid" => p.browsertrackerid = Some(value),
            "task" => p.task = Some(value),
            "placeId" | "placeid" => p.place_id = Some(value),
            "universeId" | "universeid" => p.universe_id = Some(value),
            "userId" | "userid" => p.user_id = Some(value),
            "script" => p.script = Some(value),
            _ => {}
        }
    }
    Ok(p)
}

pub fn build_player_args(p: &PlayParams) -> Vec<String> {
    let mut args = Vec::<String>::new();

    args.push("-a".into());
    args.push(format!("{}/Login/Negotiate.ashx", config::BASE_URL));

    if let Some(ticket) = &p.gameinfo {
        args.push("-t".into());
        args.push(ticket.clone());
    }
    if let Some(url) = &p.placelauncherurl {
        args.push("-j".into());
        args.push(url.clone());
    }
    if let Some(t) = &p.launchtime {
        args.push("-b".into());
        args.push(t.clone());
    }
    if let Some(v) = &p.clientversion {
        args.push("--rbxexp".into());
        args.push(v.clone());
    }
    if let Some(exp) = &p.launchexp {
        args.push("--launchexp".into());
        args.push(exp.clone());
    }
    if let Some(id) = &p.browsertrackerid {
        args.push("--browserTrackerId".into());
        args.push(id.clone());
    }
    args
}

pub fn build_studio_args(p: &PlayParams) -> Vec<String> {
    let mut args = Vec::<String>::new();

    args.push("-a".into());
    args.push(format!("{}/Login/Negotiate.ashx", config::BASE_URL));

    let task = p.task.clone().unwrap_or_else(|| match p.launchmode.as_str() {
        "" | "edit" => "EditPlace".to_string(),
        other => other.to_string(),
    });
    args.push("-task".into());
    args.push(task);

    if let Some(id) = &p.place_id {
        args.push("-placeId".into());
        args.push(id.clone());
    }
    if let Some(id) = &p.universe_id {
        args.push("-universeId".into());
        args.push(id.clone());
    }
    if let Some(id) = &p.user_id {
        args.push("-userId".into());
        args.push(id.clone());
    }
    if let Some(s) = &p.script {
        args.push("-script".into());
        args.push(s.clone());
    }
    args
}

pub fn run_pipeline(state: &Arc<Mutex<BootstrapState>>) -> Result<()> {
    let (play_url, local_file, client, kind, install_only) = {
        let s = state.lock().unwrap();
        (
            s.play_url.clone(),
            s.local_file.clone(),
            s.client.clone(),
            s.kind,
            s.install_only,
        )
    };

    let label = kind.label();

    if install_only {
        set_phase(state, Phase::Connecting, "Upgrading Memoria...", None);
    } else {
        set_phase(state, Phase::Connecting, &format!("Upgrading {label}..."), None);
    }

    // The Studio bootstrapper must not overwrite the shared player launcher with a copy of itself.
    let is_studio_bootstrapper = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()))
        .map_or(false, |n| n.contains("studiolauncher"));
    let installed_launcher = if install_only || is_studio_bootstrapper {
        installer::install_launcher_from_server()?
    } else {
        installer::install_self()?
    };
    if let Err(err) = installer::register_url_schemes(&installed_launcher) {
        log::warn!("URL scheme registration failed (non-fatal): {err:#}");
    }
    if let Err(err) = installer::register_file_associations(&installed_launcher) {
        log::warn!("file-association registration failed (non-fatal): {err:#}");
    }

    if let Err(err) = installer::enforce_global_version(state) {
        log::warn!("global version enforcement skipped: {err:#}");
    }

    if install_only {
        run_installer_pipeline(state, &client)?;
        return Ok(());
    }

    let version = installer::resolve_target_version(kind, &client)
        .with_context(|| format!("resolving version for {label} {client}"))?;
    log::info!("{label} {client} target version: {version}");

    if !installer::is_installed(kind, &client, &version) {
        set_phase(
            state,
            Phase::Configuring,
            &format!("Upgrading {label}..."),
            Some(0.0),
        );
        installer::install_client(kind, &client, &version, state)?;
    } else {
        log::info!("{label} {client} already at {version}, skipping download");
        set_progress(state, 0.95);
    }

    set_phase(state, Phase::Configuring, &format!("Upgrading {label}..."), Some(0.97));

    let exe_path = config::target_exe(kind, &client)?;
    if !exe_path.is_file() {
        return Err(anyhow!(
            "install reported success but {} is missing",
            exe_path.display()
        ));
    }

    let args = if let Some(file) = local_file.as_deref() {
        if kind != Kind::Studio {
            log::warn!("local file supplied with kind {label}; forcing Studio open");
        }
        vec![file.to_string()]
    } else {
        match play_url.as_deref() {
            Some(url) => {
                let parsed = parse_play_uri(url).context("parsing launch URI")?;
                match kind {
                    Kind::Player => build_player_args(&parsed),
                    Kind::Studio => build_studio_args(&parsed),
                }
            }
            None if kind == Kind::Studio && config::LAUNCH_WHEN_IDLE.load(Ordering::Relaxed) => {
                vec!["-a".to_string(), format!("{}/Login/Negotiate.ashx", config::BASE_URL)]
            }
            None => {
                set_phase(
                    state,
                    Phase::Idle,
                    &format!("{label} is ready. Launch from the website to start."),
                    Some(1.0),
                );
                return Ok(());
            }
        }
    };

    set_phase(state, Phase::Starting, &format!("Starting {label}..."), Some(1.0));
    log::info!("spawning {} {:?}", exe_path.display(), args);
    if let Some(dir) = exe_path.parent() {
        ensure_client_runtime_dirs(dir);
    }
    if crate::rpc::spawn_watcher(kind, &exe_path, &args).is_ok() {
        set_phase(state, Phase::Done, "Have fun!", Some(1.0));
    } else {
        spawn_target(&exe_path, &args)?;
        set_phase(state, Phase::Done, "Have fun!", Some(1.0));
    }
    std::thread::sleep(Duration::from_millis(1200));
    state.lock().unwrap().request_close = true;
    Ok(())
}

fn run_installer_pipeline(state: &Arc<Mutex<BootstrapState>>, client: &str) -> Result<()> {
    install_kind_with_overall_progress(state, Kind::Player, client, (0.0, 0.5))?;
    let _ = install_kind_with_overall_progress(state, Kind::Studio, client, (0.5, 1.0));

    set_phase(
        state,
        Phase::Idle,
        "Memoria is installed. You can now play from the website.",
        Some(1.0),
    );
    Ok(())
}

fn install_kind_with_overall_progress(
    state: &Arc<Mutex<BootstrapState>>,
    kind: Kind,
    client: &str,
    overall_range: (f32, f32),
) -> Result<()> {
    let label = kind.label();

    set_phase(
        state,
        Phase::Configuring,
        &format!("Upgrading {label}..."),
        Some(overall_range.0),
    );

    let version = installer::resolve_target_version(kind, client)
        .with_context(|| format!("resolving version for {label} {client}"))?;
    log::info!("{label} {client} target version: {version}");

    if installer::is_installed(kind, client, &version) {
        log::info!("{label} {client} already at {version}, skipping download");
        set_progress(state, overall_range.1);
        return Ok(());
    }

    installer::install_client(kind, client, &version, state)?;
    set_progress(state, overall_range.1);
    Ok(())
}

fn ensure_client_runtime_dirs(client_dir: &Path) {
    if let Some(base) = client_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
    {
        for d in ["meta", "logs", "logs/archive", "LocalStorage"] {
            let _ = std::fs::create_dir_all(base.join(d));
        }
    }
}

fn spawn_target(exe: &Path, args: &[String]) -> Result<()> {
    if let Some(client_dir) = exe.parent() {
        ensure_client_runtime_dirs(client_dir);
    }
    let mut cmd = Command::new(exe);
    cmd.args(args);
    if let Some(parent) = exe.parent() {
        cmd.current_dir(parent);
    }
    cmd.spawn()
        .with_context(|| format!("spawning {}", exe.display()))?;
    Ok(())
}

fn set_phase(
    state: &Arc<Mutex<BootstrapState>>,
    phase: Phase,
    status: &str,
    progress: Option<f32>,
) {
    let mut s = state.lock().unwrap();
    s.phase = phase;
    s.status = status.to_string();
    if let Some(p) = progress {
        s.progress = Some(p.clamp(0.0, 1.0));
    }
}

fn set_progress(state: &Arc<Mutex<BootstrapState>>, p: f32) {
    let mut s = state.lock().unwrap();
    s.progress = Some(p.clamp(0.0, 1.0));
}

#[cfg(windows)]
pub fn client_already_running() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let root_lc = match config::install_root() {
        Ok(p) => p.to_string_lossy().to_ascii_lowercase(),
        Err(_) => return false,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut running = false;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]).to_ascii_lowercase();
                if name == "robloxplayerbeta.exe" && process_image_under(entry.th32ProcessID, &root_lc) {
                    running = true;
                    break;
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        running
    }
}

#[cfg(windows)]
unsafe fn process_image_under(pid: u32, root_lc: &str) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if handle == 0 {
        return false;
    }
    let mut buf = vec![0u16; 32768];
    let mut size = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
    CloseHandle(handle);
    if ok == 0 {
        return false;
    }
    let path = String::from_utf16_lossy(&buf[..size as usize]).to_ascii_lowercase();
    path.starts_with(root_lc)
}

#[cfg(not(windows))]
pub fn client_already_running() -> bool {
    false
}
