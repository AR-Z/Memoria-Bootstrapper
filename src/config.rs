
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use directories::BaseDirs;

pub const BASE_URL: &str = "https://api.memora.zip";
pub const APP_NAME: &str = "Memoria";
pub const LAUNCHER_EXE: &str = "MemoriaPlayerLauncher.exe";

pub const DEFAULT_CLIENT: &str = "2021";

/// Set by --launch: with no link to open, start the client anyway instead of only installing it.
pub static LAUNCH_WHEN_IDLE: AtomicBool = AtomicBool::new(false);

pub const LAUNCHER_VERSION: &str = "3";

pub const URL_SCHEMES_PLAYER: &[&str] = &["memoria-player"];
pub const URL_SCHEMES_STUDIO: &[&str] = &["memoria-studio"];

pub const STUDIO_FILE_EXTS: &[&str] = &["memoriarbxm"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Player,
    Studio,
}

impl Kind {
    pub fn from_scheme(scheme: &str) -> Option<Self> {
        let s = scheme.to_ascii_lowercase();
        if URL_SCHEMES_PLAYER.iter().any(|p| *p == s) {
            Some(Kind::Player)
        } else if URL_SCHEMES_STUDIO.iter().any(|p| *p == s) {
            Some(Kind::Studio)
        } else {
            None
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Player => "Memoria",
            Kind::Studio => "Memoria Studio",
        }
    }

    pub fn exe_name(self) -> &'static str {
        match self {
            Kind::Player => "RobloxPlayerBeta.exe",
            Kind::Studio => "RobloxStudioBeta.exe",
        }
    }
}

pub fn install_root() -> anyhow::Result<PathBuf> {
    let dirs = BaseDirs::new().ok_or_else(|| anyhow::anyhow!("no LOCALAPPDATA"))?;
    Ok(dirs.data_local_dir().join(APP_NAME))
}

pub fn launcher_exe_path() -> anyhow::Result<PathBuf> {
    Ok(install_root()?.join(LAUNCHER_EXE))
}

pub fn kind_root(kind: Kind) -> anyhow::Result<PathBuf> {
    let sub = match kind {
        Kind::Player => "clients",
        Kind::Studio => "Studio",
    };
    Ok(install_root()?.join(sub))
}

pub fn install_dir(kind: Kind, client: &str) -> anyhow::Result<PathBuf> {
    Ok(kind_root(kind)?.join(client))
}

pub fn target_exe(kind: Kind, client: &str) -> anyhow::Result<PathBuf> {
    Ok(install_dir(kind, client)?.join(kind.exe_name()))
}

pub fn state_dir() -> anyhow::Result<PathBuf> {
    Ok(install_root()?.join("state"))
}

pub fn cached_version_file(kind: Kind, client: &str) -> anyhow::Result<PathBuf> {
    let prefix = match kind {
        Kind::Player => "LATEST",
        Kind::Studio => "LATEST-studio",
    };
    Ok(state_dir()?.join(format!("{prefix}-{client}")))
}

pub fn version_url(kind: Kind, client: &str) -> String {
    match kind {
        Kind::Player => format!("{BASE_URL}/setup/{client}/version.txt"),
        Kind::Studio => format!("{BASE_URL}/setup/studio/{client}/version.txt"),
    }
}

pub fn bundle_url(kind: Kind, client: &str, version: &str) -> String {
    match kind {
        Kind::Player => format!("{BASE_URL}/setup/{client}/{version}-client.zip"),
        Kind::Studio => format!("{BASE_URL}/setup/studio/{client}/{version}-studio.zip"),
    }
}

pub fn launcher_url() -> String {
    format!("{BASE_URL}/setup/launcher.exe")
}

pub fn launcher_version_url() -> String {
    format!("{BASE_URL}/setup/launcher/version.txt")
}

pub fn global_version_url() -> String {
    format!("{BASE_URL}/version")
}

pub fn global_version_file() -> anyhow::Result<PathBuf> {
    Ok(state_dir()?.join("GLOBAL-VERSION"))
}
