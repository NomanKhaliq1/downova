use crate::schedule::FinishAction;
use std::process::Command;

/// Carry out a queue's finish action. `Exit` is left to the app (it owns the process), so it returns false for it.
/// Shutdown waits 60 s so the user can cancel with `shutdown /a`.
/// Not covered by tests on purpose: running them would sleep or shut down the machine.
#[cfg(windows)]
pub fn perform(a: &FinishAction) -> bool {
    let spawned = match a {
        FinishAction::None | FinishAction::Exit => return false,
        FinishAction::Sleep => Command::new("rundll32.exe").args(["powrprof.dll,SetSuspendState", "0,1,0"]).spawn(),
        FinishAction::Hibernate => Command::new("shutdown").args(["/h"]).spawn(),
        FinishAction::Shutdown => Command::new("shutdown").args(["/s", "/t", "60"]).spawn(),
        FinishAction::Command(c) => Command::new("cmd").args(["/C", c]).spawn(),
    };
    spawned.is_ok()
}

#[cfg(not(windows))]
pub fn perform(a: &FinishAction) -> bool {
    match a {
        FinishAction::Command(c) => Command::new("sh").args(["-c", c]).spawn().is_ok(),
        _ => false,
    }
}
