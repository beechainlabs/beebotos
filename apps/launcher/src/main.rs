#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    {
        beebotos_launcher::windows::run();
    }

    #[cfg(not(windows))]
    {
        eprintln!("BeeBotOS Launcher is only available on Windows.");
    }
}
