//! Locks the vault when the screen locks or the computer goes to sleep. It listens to the system's message bus
//! (logind and the desktop's screen saver) through `dbus-monitor`, which is on every desktop; with no `dbus-monitor`
//! it does nothing, and the vault still locks itself when idle.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;

/// What the lines of `dbus-monitor` say, one by one: `true` when the screen has locked or the machine is going to sleep.
#[derive(Default)]
pub struct LockLines {
    /// The signal whose boolean argument we are waiting for.
    waiting_for_bool: Option<&'static str>,
    /// `PropertiesChanged` is a lock only when it is `LockedHint` that changed.
    saw_locked_hint: bool,
    /// The properties that changed belong to a login session (not to something else).
    for_session: bool,
}

impl LockLines {
    pub fn feed(&mut self, line: &str) -> bool {
        let l = line.trim();
        if l.starts_with("signal ") || l.starts_with("method call") || l.starts_with("method return") {
            self.waiting_for_bool = None;
            self.saw_locked_hint = false;
            self.for_session = false;
            // logind's `Lock` signal (loginctl lock-session, and what many desktops use)
            if l.contains("interface=org.freedesktop.login1.Session") && l.contains("member=Lock") && !l.contains("member=Locked") {
                return true;
            }
            if l.contains("member=PrepareForSleep") {
                self.waiting_for_bool = Some("sleep");
            } else if l.contains("member=ActiveChanged") && (l.contains("ScreenSaver") || l.contains("screensaver")) {
                self.waiting_for_bool = Some("screensaver");
            } else if l.contains("member=PropertiesChanged") {
                self.waiting_for_bool = Some("properties");
            }
            return false;
        }
        match self.waiting_for_bool {
            Some("sleep" | "screensaver") if l.starts_with("boolean ") => {
                self.waiting_for_bool = None;
                l == "boolean true"
            }
            Some("properties") => {
                if l.contains("\"org.freedesktop.login1.Session\"") {
                    self.for_session = true;
                } else if l.contains("\"LockedHint\"") {
                    self.saw_locked_hint = self.for_session;
                } else if self.saw_locked_hint && l.starts_with("variant") && l.ends_with("boolean true") {
                    self.saw_locked_hint = false;
                    self.waiting_for_bool = None;
                    return true;
                } else if self.saw_locked_hint && l.starts_with("variant") {
                    self.saw_locked_hint = false;
                }
                false
            }
            _ => false,
        }
    }
}

fn watch(bus: &'static str, rules: &[&str], on_lock: Arc<dyn Fn() + Send + Sync>) {
    let Ok(mut child) = Command::new("dbus-monitor").arg(format!("--{bus}")).args(rules).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return;
    };
    let Some(out) = child.stdout.take() else { return };
    std::thread::spawn(move || {
        let mut lines = LockLines::default();
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if lines.feed(&line) {
                on_lock();
            }
        }
        let _ = child.wait();
    });
}

/// Starts listening; `on_lock` runs each time the screen locks or the machine suspends.
pub fn start(on_lock: Arc<dyn Fn() + Send + Sync>) {
    watch(
        "system",
        &["type='signal',interface='org.freedesktop.login1.Session',member='Lock'", "type='signal',interface='org.freedesktop.login1.Manager',member='PrepareForSleep'", "type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',arg0='org.freedesktop.login1.Session'"],
        Arc::clone(&on_lock),
    );
    watch("session", &["type='signal',interface='org.gnome.ScreenSaver',member='ActiveChanged'", "type='signal',interface='org.freedesktop.ScreenSaver',member='ActiveChanged'", "type='signal',interface='org.kde.screensaver',member='ActiveChanged'"], on_lock);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `dbus-monitor` prints for each of the events (as it does on a desktop of today).
    fn locks(text: &str) -> usize {
        let mut l = LockLines::default();
        text.lines().filter(|line| l.feed(line)).count()
    }

    #[test]
    fn a_session_lock_signal_locks() {
        let t = "signal time=1760.5 sender=:1.5 -> destination=(null destination) serial=33 path=/org/freedesktop/login1/session/_32; interface=org.freedesktop.login1.Session; member=Lock\n";
        assert_eq!(locks(t), 1);
        assert_eq!(locks("signal time=1 sender=:1.5 -> destination=(null destination) serial=3 path=/org/freedesktop/login1/session/_32; interface=org.freedesktop.login1.Session; member=Unlock\n"), 0);
    }

    #[test]
    fn going_to_sleep_locks_but_waking_up_does_not() {
        let sleep = "signal time=9 sender=:1.5 -> destination=(null destination) serial=7 path=/org/freedesktop/login1; interface=org.freedesktop.login1.Manager; member=PrepareForSleep\n   boolean true\n";
        let wake = "signal time=9 sender=:1.5 -> destination=(null destination) serial=7 path=/org/freedesktop/login1; interface=org.freedesktop.login1.Manager; member=PrepareForSleep\n   boolean false\n";
        assert_eq!((locks(sleep), locks(wake)), (1, 0));
    }

    #[test]
    fn the_desktops_screen_saver_locks_when_it_becomes_active() {
        let on = "signal time=5 sender=:1.40 -> destination=(null destination) serial=88 path=/org/gnome/ScreenSaver; interface=org.gnome.ScreenSaver; member=ActiveChanged\n   boolean true\n";
        let off = on.replace("boolean true", "boolean false");
        assert_eq!((locks(on), locks(&off)), (1, 0));
        let kde = "signal time=5 sender=:1.40 -> destination=(null destination) serial=88 path=/ScreenSaver; interface=org.freedesktop.ScreenSaver; member=ActiveChanged\n   boolean true\n";
        assert_eq!(locks(kde), 1);
    }

    #[test]
    fn a_locked_hint_that_turns_on_locks_and_other_properties_do_not() {
        let head = "signal time=5 sender=:1.5 -> destination=(null destination) serial=9 path=/org/freedesktop/login1/session/_32; interface=org.freedesktop.DBus.Properties; member=PropertiesChanged\n   string \"org.freedesktop.login1.Session\"\n   array [\n      dict entry(\n         string \"LockedHint\"\n";
        assert_eq!(locks(&format!("{head}            variant                boolean true\n         )\n      ]\n")), 1);
        assert_eq!(locks(&format!("{head}            variant                boolean false\n         )\n      ]\n")), 0);
        let other = "signal time=5 sender=:1.5 -> destination=(null destination) serial=9 path=/org/freedesktop/login1/session/_32; interface=org.freedesktop.DBus.Properties; member=PropertiesChanged\n   string \"org.freedesktop.login1.Session\"\n   array [\n      dict entry(\n         string \"IdleHint\"\n            variant                boolean true\n         )\n      ]\n";
        assert_eq!(locks(other), 0, "IdleHint is not a lock");
    }

    #[test]
    fn noise_between_events_is_ignored_and_state_does_not_leak() {
        let t = "method call time=1 sender=:1.1 -> destination=org.freedesktop.DBus serial=1 path=/; interface=org.freedesktop.DBus; member=Hello\nsignal time=2 sender=:1.5 -> destination=(null destination) serial=4 path=/x; interface=org.freedesktop.login1.Manager; member=PrepareForSleep\n   boolean false\nsignal time=3 sender=:1 -> destination=(null destination) serial=5 path=/y; interface=a.b; member=Other\n   boolean true\n";
        assert_eq!(locks(t), 0);
    }
}
