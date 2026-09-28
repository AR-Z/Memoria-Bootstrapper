
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    Memoria_bootstrapper::run(args, false)
}
