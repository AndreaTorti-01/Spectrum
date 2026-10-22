//! Small first-run dialog offering "Install" or "Just run", with a "Start with Windows" checkbox.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallChoice {
    Install { autostart: bool },
    JustRun,
    Cancel,
}

/// Modal; runs its own message loop.
pub fn run() -> InstallChoice {
    todo!("ui::install_window::run")
}
