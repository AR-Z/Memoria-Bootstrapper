#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = vec!["--studio".to_string(), "--launch".to_string()];
    args.extend(std::env::args().skip(1));
    Memoria_bootstrapper::run(args, false)
}
