// Keep the helper invisible when launched with inherited IPC handles.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    std::process::exit(vyra_lib::watchdog::main_entry());
}
