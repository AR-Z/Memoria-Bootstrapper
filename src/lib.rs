
pub mod config;
pub mod installer;
pub mod launcher;
pub mod rpc;
pub mod ui;

use std::sync::{Arc, Mutex};

use anyhow::Result;
use eframe::egui;

const WINDOW_W: f32 = 480.0;
const WINDOW_H: f32 = 250.0;

pub fn run(args: Vec<String>, force_install_only: bool) -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    match args.first().map(String::as_str) {
        Some("--rpc-play") => return rpc::watch_and_play(config::Kind::Player, &args[1..]),
        Some("--rpc-studio") => return rpc::watch_and_play(config::Kind::Studio, &args[1..]),
        _ => {}
    }

    let mut input = launcher::parse_argv(&args);
    if force_install_only {
        input.install_only = true;
        input.play_url = None;
        input.local_file = None;
    }

    config::LAUNCH_WHEN_IDLE.store(input.launch_when_idle, std::sync::atomic::Ordering::Relaxed);

    if !input.install_only
        && input.kind == config::Kind::Player
        && input.play_url.is_some()
        && launcher::client_already_running()
    {
        log::info!("Memoria client already running; refusing second instance");
        return Ok(());
    }

    if installer::self_update_if_needed(&args) {
        return Ok(());
    }

    let title = if input.install_only {
        "Memoria Installer".to_string()
    } else {
        input.kind.label().to_string()
    };

    let state = Arc::new(Mutex::new(ui::BootstrapState::new(
        input.play_url,
        input.local_file,
        input.client,
        input.kind,
        input.install_only,
    )));

    {
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            if let Err(err) = launcher::run_pipeline(&state) {
                log::error!("launch pipeline failed: {err:#}");
                let mut s = state.lock().unwrap();
                s.fail(format!("Launch failed: {err:#}"));
            }
        });
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([WINDOW_W, WINDOW_H])
        .with_min_inner_size([WINDOW_W, WINDOW_H])
        .with_max_inner_size([WINDOW_W, WINDOW_H])
        .with_resizable(false)
        .with_decorations(false)
        .with_transparent(false)
        .with_title(&title);

    if let Some((rgba, w, h)) = ui::app_icon_rgba() {
        viewport = viewport.with_icon(egui::IconData {
            rgba,
            width: w,
            height: h,
        });
    }

    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "Memoria",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Box::new(ui::BootstrapApp::new(cc, state))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))?;

    Ok(())
}
