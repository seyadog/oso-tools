//! Which programs run inside a terminal of oso. Every process started in a local terminal inherits `OSO_PANE=<session>`
//! (the background server sets it), so the processes of a terminal are the ones that carry it in their environment.
//! Linux only (`/proc`), same user only (the kernel hides the environment of other users' processes), and read only.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub argv: Vec<String>,
}

/// The processes that carry `OSO_PANE=<n>`, by terminal.
pub fn scan_panes() -> HashMap<u64, Vec<Proc>> {
    scan_panes_in(Path::new("/proc"))
}

pub fn scan_panes_in(root: &Path) -> HashMap<u64, Vec<Proc>> {
    let mut out: HashMap<u64, Vec<Proc>> = HashMap::new();
    let Ok(dir) = std::fs::read_dir(root) else { return out };
    for entry in dir.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        let p = entry.path();
        let Ok(env) = std::fs::read(p.join("environ")) else { continue };
        let Some(pane) = env.split(|b| *b == 0).find_map(|kv| kv.strip_prefix(b"OSO_PANE=")).and_then(|v| std::str::from_utf8(v).ok()).and_then(|v| v.parse::<u64>().ok()) else { continue };
        let argv: Vec<String> = std::fs::read(p.join("cmdline")).map(|c| c.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect()).unwrap_or_default();
        if argv.is_empty() {
            continue; // a kernel thread or a zombie
        }
        let ppid = std::fs::read_to_string(p.join("stat")).ok().and_then(|s| parse_ppid(&s)).unwrap_or(0);
        out.entry(pane).or_default().push(Proc { pid, ppid, argv });
    }
    out
}

/// The parent pid out of `/proc/<pid>/stat` (the command name, in parentheses, may hold spaces and parentheses).
fn parse_ppid(stat: &str) -> Option<u32> {
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(1)?.parse().ok()
}

pub fn cwd_of(root: &Path, pid: u32) -> Option<PathBuf> {
    std::fs::read_link(root.join(pid.to_string()).join("cwd")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn the_parent_is_read_even_when_the_name_has_spaces_and_parentheses() {
        assert_eq!(parse_ppid("123 (my (odd) prog) S 77 1 1 0"), Some(77));
        assert_eq!(parse_ppid("garbage"), None);
    }

    #[test]
    fn a_process_of_a_terminal_is_found_by_the_pane_it_carries_and_no_other_is() {
        let mut mine = Command::new("sleep").arg("30").env("OSO_PANE", "4242").stdout(Stdio::null()).spawn().unwrap();
        let mut other = Command::new("sleep").arg("31").env("OSO_PANE", "4343").spawn().unwrap();
        let mut plain = Command::new("sleep").arg("32").env_remove("OSO_PANE").spawn().unwrap();
        let found = scan_panes();
        let in_pane = |pane: u64, pid: u32| found.get(&pane).is_some_and(|v| v.iter().any(|p| p.pid == pid));
        assert!(in_pane(4242, mine.id()) && in_pane(4343, other.id()));
        assert!(!in_pane(4242, other.id()) && !in_pane(4343, mine.id()));
        assert!(!found.values().flatten().any(|p| p.pid == plain.id()), "a process outside every terminal is not listed");
        let p = found[&4242].iter().find(|p| p.pid == mine.id()).unwrap();
        assert_eq!(p.argv, ["sleep", "30"]);
        assert_eq!(p.ppid, std::process::id(), "its parent is this test");
        assert!(cwd_of(Path::new("/proc"), mine.id()).is_some());
        for c in [&mut mine, &mut other, &mut plain] {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}
