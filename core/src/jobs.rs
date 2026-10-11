//! Helper processes that run beside the terminals: a tunnel (`ssh -N -L …`) or a file copy (`scp`). They run in a
//! terminal of their own that nobody sees, so the same saved passwords are typed at their prompts, and what they
//! print can be read to tell how they are doing.

use crate::session::{PtySession, SpawnOpts};
use anyhow::Result;
use std::time::{Duration, Instant};

pub struct Job {
    pty: PtySession,
    started: Instant,
    code: Option<u32>,
}

impl Job {
    pub fn spawn(opts: SpawnOpts) -> Result<Job> {
        Ok(Job { pty: PtySession::spawn_with(opts)?, started: Instant::now(), code: None })
    }

    /// The exit code once the process has ended.
    pub fn poll(&mut self) -> Option<u32> {
        if self.code.is_none() {
            self.pty.poll_exit();
            self.code = self.pty.exit_code;
        }
        self.code
    }

    pub fn age(&self) -> Duration {
        self.started.elapsed()
    }

    /// Everything printed so far (kept to a limit by the session).
    pub fn output(&self) -> String {
        String::from_utf8_lossy(&self.pty.history()).replace('\r', "\n")
    }

    /// What is on the (invisible) screen right now: the progress line of scp lives here.
    pub fn screen(&self) -> String {
        self.pty.with_screen(|s| s.contents())
    }

    pub fn kill(&mut self) {
        self.pty.kill();
    }
}

/// The last `NN%` in a text (scp's progress line), as a number.
pub fn percent(text: &str) -> Option<u8> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = None;
    for (i, c) in chars.iter().enumerate() {
        if *c != '%' {
            continue;
        }
        let mut start = i;
        while start > 0 && chars[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if let Ok(n) = chars[start..i].iter().collect::<String>().parse::<u16>() {
            if n <= 100 {
                found = Some(n as u8);
            }
        }
    }
    found
}

/// Whether the process is stuck at a question it cannot answer by itself (a password that is not saved, a key to accept).
pub fn stuck_at(output: &str) -> Option<&'static str> {
    let t = output.trim_end().to_lowercase();
    if t.ends_with("(yes/no)?") || t.ends_with("(yes/no/[fingerprint])?") || t.contains("are you sure you want to continue connecting") && t.ends_with('?') {
        return Some("This server's key is not known yet: open it once in a terminal and accept it, then try again.");
    }
    if t.ends_with("password:") || (t.contains("passphrase for") && t.ends_with(':')) {
        return Some("It needs a password: save it in the vault and unlock the vault.");
    }
    None
}

/// A sentence for what went wrong, from what the process printed (its own debug lines left out).
pub fn problem(output: &str) -> String {
    let lines: Vec<&str> = output.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("debug") && !l.starts_with("OpenSSH_")).collect();
    let all = lines.join("\n").to_lowercase();
    if all.contains("permission denied") || all.contains("authentication failed") {
        return "The server did not accept the login.".into();
    }
    if all.contains("host key verification failed") || all.contains("remote host identification has changed") {
        return "The server's key does not match the saved one: open it in a terminal to see why.".into();
    }
    if all.contains("address already in use") || all.contains("cannot listen to port") || all.contains("could not request local forwarding") {
        return "That port is already in use on this computer: pick another one.".into();
    }
    if all.contains("remote port forwarding failed") {
        return "The server would not open that port.".into();
    }
    lines.last().map(|l| l.chars().take(200).collect()).unwrap_or_else(|| "It stopped without saying why.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_scp_progress() {
        assert_eq!(percent("file.bin   45%   12MB   3.1MB/s   00:04 ETA"), Some(45));
        assert_eq!(percent("a 3%  \n b 100%  x"), Some(100));
        assert_eq!(percent("no progress here"), None);
        assert_eq!(percent("100% of the time"), Some(100));
        assert_eq!(percent("%"), None);
    }

    #[test]
    fn knows_when_it_is_stuck() {
        assert!(stuck_at("root@h's password: ").is_some());
        assert!(stuck_at("Enter passphrase for key '/k/id': ").is_some());
        assert!(stuck_at("The authenticity of host 'h' can't be established.\nAre you sure you want to continue connecting (yes/no/[fingerprint])? ").is_some());
        assert!(stuck_at("debug1: Authenticated to h\n").is_none());
        assert!(stuck_at("").is_none());
    }

    #[test]
    fn says_what_went_wrong_in_plain_words() {
        assert!(problem("debug1: x\nroot@h: Permission denied (publickey).").contains("did not accept"));
        assert!(problem("bind [127.0.0.1]:5433: Address already in use\nchannel_setup_fwd_listener_tcpip: cannot listen to port: 5433").contains("already in use"));
        assert_eq!(problem("debug1: a\nssh: connect to host h port 22: Connection refused"), "ssh: connect to host h port 22: Connection refused");
        assert!(problem("").contains("without saying"));
    }
}
