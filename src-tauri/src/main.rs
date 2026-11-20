// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--watchdog-self-check") {
        std::process::exit(vyra_lib::watchdog::installation_check());
    }
    vyra_lib::run()
}
