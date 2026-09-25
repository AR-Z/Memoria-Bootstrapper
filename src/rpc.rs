use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

use crate::config::Kind;

const DISCORD_APP_ID: &str = "1548240206254182450";
const LOGO_URL: &str = "https://api.memora.zip/setup/rpc/icon.png";
const REVIVAL_NAME: &str = "Memoria";
const WEBSITE_URL: &str = "https://memora.zip";
const PLACE_NAME_URL: &str = "https://www.memora.zip/client/place-name";
const USER_AGENT: &str = "MemoriaBootstrapper/0.1 (+https://memora.zip)";

pub struct Presence {
    client: DiscordIpcClient,
}

impl Presence {
    pub fn connect() -> Option<Presence> {
        if DISCORD_APP_ID.is_empty() {
            return None;
        }
        let mut client = DiscordIpcClient::new(DISCORD_APP_ID).ok()?;
        client.connect().ok()?;
        Some(Presence { client })
    }

    pub fn set_activity(&mut self, details: &str, state: &str) {
        let start = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let activity = activity::Activity::new()
            .details(details)
            .state(state)
            .assets(
                activity::Assets::new()
                    .large_image(LOGO_URL)
                    .large_text(REVIVAL_NAME),
            )
            .timestamps(activity::Timestamps::new().start(start))
            .buttons(vec![activity::Button::new("Visit Memoria", WEBSITE_URL)]);
        let _ = self.client.set_activity(activity);
    }

    pub fn clear(&mut self) {
        let _ = self.client.clear_activity();
        let _ = self.client.close();
    }
}

pub fn spawn_watcher(kind: Kind, game_exe: &Path, game_args: &[String]) -> std::io::Result<()> {
    let self_exe = std::env::current_exe()?;
    let mut cmd = Command::new(self_exe);
    cmd.arg(watcher_flag(kind)).arg(game_exe).args(game_args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(DETACHED_PROCESS);
    }
    cmd.spawn()?;
    Ok(())
}

pub fn watcher_flag(kind: Kind) -> &'static str {
    match kind {
        Kind::Player => "--rpc-play",
        Kind::Studio => "--rpc-studio",
    }
}

pub fn watch_and_play(kind: Kind, rest: &[String]) -> anyhow::Result<()> {
    let exe = rest
        .first()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("rpc watcher requires a client path"))?;
    let client_args = &rest[1..];
    let mut cmd = Command::new(&exe);
    cmd.args(client_args);
    if let Some(parent) = exe.parent() {
        cmd.current_dir(parent);
    }
    let mut child = cmd.spawn()?;

    let place_name = find_place_id(client_args).and_then(|id| fetch_place_name(&id));
    let (details, state) = describe(kind, place_name);

    let mut presence = Presence::connect();
    if let Some(p) = presence.as_mut() {
        p.set_activity(&details, &state);
    }
    let _ = child.wait();
    if let Some(p) = presence.as_mut() {
        p.clear();
    }
    Ok(())
}

fn describe(kind: Kind, place_name: Option<String>) -> (String, String) {
    match kind {
        Kind::Player => (
            place_name.unwrap_or_else(|| "Playing Memoria".to_string()),
            "Playing on Memoria".to_string(),
        ),
        Kind::Studio => (
            "Memoria Studio".to_string(),
            match place_name {
                Some(name) => format!("Editing {name}"),
                None => "In the editor".to_string(),
            },
        ),
    }
}

fn find_place_id(args: &[String]) -> Option<String> {
    for (i, arg) in args.iter().enumerate() {
        let lower = arg.to_ascii_lowercase();
        if lower == "-placeid" {
            if let Some(next) = args.get(i + 1) {
                if !next.is_empty() && next.chars().all(|c| c.is_ascii_digit()) {
                    return Some(next.clone());
                }
            }
        }
        if let Some(pos) = lower.find("placeid=") {
            let digits: String = arg[pos + 8..].chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                return Some(digits);
            }
        }
    }
    None
}

fn fetch_place_name(place_id: &str) -> Option<String> {
    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(4))
        .user_agent(USER_AGENT)
        .build()
        .ok()?;
    let body = http
        .get(format!("{PLACE_NAME_URL}?placeId={place_id}"))
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .ok()?;
    let name = extract_json_string(&body, "name")?;
    let name = name.trim();
    if name.len() < 2 {
        return None;
    }
    Some(name.chars().take(100).collect())
}

fn extract_json_string(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = body.find(&needle)? + needle.len();
    let mut out = String::new();
    let mut chars = body[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        out.push(ch);
                    }
                }
                'n' | 't' | 'r' => out.push(' '),
                other => out.push(other),
            },
            _ => out.push(c),
        }
    }
    None
}
