//! The record of what AI agents asked and what happened: `audit.jsonl`, one line each, only ever appended to.
//! It holds the command the agent sent and the first lines of what came back, with every secret of the vault blanked
//! out of both (the sudo password is not in it).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(default)]
pub struct Entry {
    /// Unix seconds.
    pub ts: u64,
    /// The request this line belongs to (its "allowed" line and its "finished" line share it). 0 in old logs.
    pub id: u64,
    pub pane: u64,
    pub agent: String,
    pub action: String,
    pub host: String,
    pub command: String,
    /// `allowed`, `allowed_without_asking`, `denied`, `blocked`, `failed`…
    pub decision: String,
    pub exit_code: Option<i32>,
    pub bytes_out: Option<usize>,
    pub note: String,
    /// The server's policy when it was asked: `ask`, `read_only_auto` or `blocked` (empty when none applies).
    pub policy: String,
    /// Why it was let through, asked or refused, in words.
    pub reason: String,
    /// The first lines of what the command printed (already blanked of secrets).
    pub preview: String,
    /// Everything it printed, up to `MAX_OUTPUT` bytes (blanked of secrets). Not part of the list the panel polls:
    /// it is read on demand by `find_detail`.
    pub output: String,
}

/// How much of a command's output is kept in the log (the agent got at most this much of each stream too).
pub const MAX_OUTPUT: usize = 64 * 1024;

pub struct Audit {
    path: PathBuf,
}

impl Audit {
    pub fn new(path: PathBuf) -> Audit {
        Audit { path }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Adds a line. A failure to write is not a reason to refuse what was approved, but it is said on stderr.
    pub fn log(&self, mut e: Entry) {
        if e.ts == 0 {
            e.ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        }
        let Ok(mut line) = serde_json::to_vec(&e) else { return };
        line.push(b'\n');
        let result = (|| -> std::io::Result<()> {
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
            }
            f.write_all(&line)
        })();
        if let Err(err) = result {
            eprintln!("oso: could not write the audit log: {err}");
        }
    }
}

/// How much of the end of the log is read to build the panel (the log only grows).
const TAIL_BYTES: u64 = 2 * 1024 * 1024;

/// The last lines of the log, oldest first. A line that cannot be read is skipped.
pub fn recent(path: &std::path::Path, max_lines: usize) -> Vec<Entry> {
    recent_tail(path, max_lines, TAIL_BYTES)
}

fn recent_tail(path: &std::path::Path, max_lines: usize, tail: u64) -> Vec<Entry> {
    let Ok(mut f) = std::fs::File::open(path) else { return vec![] };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(tail);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return vec![];
    }
    let mut raw = Vec::new();
    if f.read_to_end(&mut raw).is_err() {
        return vec![];
    }
    let text = String::from_utf8_lossy(&raw);
    let mut lines = text.lines();
    if start > 0 {
        lines.next(); // it starts in the middle of a line
    }
    let all: Vec<Entry> = lines.filter_map(|l| serde_json::from_str(l).ok()).collect();
    all[all.len().saturating_sub(max_lines)..].to_vec()
}

/// One thing an agent asked, with how it was decided and how it ended: what the activity panel shows.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Activity {
    pub id: u64,
    /// When it was asked, and when anything last happened to it (Unix seconds).
    pub ts: u64,
    pub updated: u64,
    pub pane: u64,
    pub agent: String,
    pub action: String,
    pub host: String,
    pub command: String,
    /// How it was decided: `allowed` (you said yes), `allowed_and_remembered_for_reads`, `allowed_without_asking`
    /// (automatic), `denied`, `blocked`.
    pub decision: String,
    pub policy: String,
    pub reason: String,
    /// `running`, `finished`, `failed`, `denied`, `blocked`, `done` (nothing to wait for) or `unknown` (it never reported back).
    pub state: String,
    pub exit_code: Option<i32>,
    pub bytes_out: Option<usize>,
    pub preview: String,
    /// There is more to see than `preview`: `find_detail` has the whole command and output.
    pub has_output: bool,
    pub note: String,
}

/// Actions that run something; the others (a tunnel, a list of servers) have no end to wait for.
fn runs(action: &str) -> bool {
    matches!(action, "run" | "sudo" | "run_with")
}

fn start_state(e: &Entry) -> &'static str {
    match e.decision.as_str() {
        "denied" => "denied",
        "blocked" => "blocked",
        "failed" => "failed",
        "finished" => "finished",
        _ if runs(&e.action) => "running",
        _ => "done",
    }
}

/// Joins the lines of one request into one thing, oldest first. `context` and `hosts` (reading the list of servers)
/// run nothing and are left out. Lines of old logs have no id: a "finished" line goes with the last open request
/// that has the same pane, host and command. A request that never reported back within 15 minutes is `unknown`.
pub fn activities(entries: &[Entry], now: u64) -> Vec<Activity> {
    let mut out: Vec<Activity> = vec![];
    let mut by_id: HashMap<u64, usize> = HashMap::new();
    for e in entries {
        if matches!(e.action.as_str(), "context" | "hosts") {
            continue;
        }
        let ends = matches!(e.decision.as_str(), "finished" | "failed");
        let at = if e.id != 0 {
            by_id.get(&e.id).copied()
        } else if ends {
            out.iter().rposition(|a| a.id == 0 && a.state == "running" && (a.pane, &a.host, &a.command, &a.action) == (e.pane, &e.host, &e.command, &e.action))
        } else {
            None
        };
        match at {
            Some(i) if ends => {
                let a = &mut out[i];
                a.state = start_state(e).into();
                a.updated = e.ts;
                a.exit_code = e.exit_code;
                a.bytes_out = e.bytes_out;
                a.preview = e.preview.clone();
                a.has_output |= !e.output.is_empty();
                a.note = e.note.clone();
            }
            Some(i) => {
                // a later line of the same request that is not an end (rare): keep the newest words
                let a = &mut out[i];
                a.updated = e.ts;
                if !e.reason.is_empty() {
                    a.reason = e.reason.clone();
                }
            }
            None => {
                if e.id != 0 {
                    by_id.insert(e.id, out.len());
                }
                out.push(Activity {
                    id: e.id,
                    ts: e.ts,
                    updated: e.ts,
                    pane: e.pane,
                    agent: e.agent.clone(),
                    action: e.action.clone(),
                    host: e.host.clone(),
                    command: e.command.clone(),
                    decision: if ends { "allowed".into() } else { e.decision.clone() },
                    policy: e.policy.clone(),
                    reason: e.reason.clone(),
                    state: start_state(e).into(),
                    exit_code: e.exit_code,
                    bytes_out: e.bytes_out,
                    preview: e.preview.clone(),
                    has_output: !e.output.is_empty(),
                    note: e.note.clone(),
                });
            }
        }
    }
    for a in &mut out {
        if a.state == "running" && now.saturating_sub(a.updated) > 900 {
            a.state = "unknown".into();
        }
    }
    out
}

/// The last `limit` things agents asked, oldest first (what the panel shows).
pub fn recent_activity(path: &std::path::Path, limit: usize) -> Vec<Activity> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let all = activities(&recent(path, limit.saturating_mul(4).max(40)), now);
    all[all.len().saturating_sub(limit)..].to_vec()
}

/// The first lines of what a command printed, short enough to keep in the log: stdout, then stderr.
pub fn preview_of(stdout: &str, stderr: &str) -> String {
    const LINES: usize = 8;
    const CHARS: usize = 800;
    let mut all = stdout.lines().chain(stderr.lines());
    let mut out: String = all.by_ref().take(LINES).collect::<Vec<_>>().join("\n");
    let mut cut = all.next().is_some();
    if out.chars().count() > CHARS {
        out = out.chars().take(CHARS).collect();
        cut = true;
    }
    if cut {
        out.push('…');
    }
    out
}

/// The whole command and the whole output of one request (what *Details* shows). The log is read from the top: this
/// is asked for when a person opens one card, not on every refresh. `None` for a request of an old log (no `id`)
/// or one that is not in the log.
pub fn find_detail(path: &std::path::Path, id: u64) -> Option<Detail> {
    if id == 0 {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let needle = format!("\"id\":{id},");
    let mut detail: Option<Detail> = None;
    for line in std::io::BufRead::lines(std::io::BufReader::new(file)).map_while(Result::ok) {
        if !line.contains(&needle) {
            continue;
        }
        let Ok(e) = serde_json::from_str::<Entry>(&line) else { continue };
        if e.id != id {
            continue;
        }
        let d = detail.get_or_insert_with(|| Detail { command: e.command.clone(), output: String::new(), cut: false });
        if !e.output.is_empty() {
            d.output = e.output.clone();
            d.cut = e.output.ends_with(OUTPUT_CUT);
        }
    }
    detail
}

/// What the end of a cut output says.
pub const OUTPUT_CUT: &str = "\n[… cut here: oso keeps the first 64 KB]";

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Detail {
    pub command: String,
    pub output: String,
    pub cut: bool,
}

/// `stdout` then `stderr`, as one text of at most `MAX_OUTPUT` bytes, ending in `OUTPUT_CUT` when it was cut.
pub fn output_of(stdout: &str, stderr: &str) -> String {
    let mut all = String::from(stdout);
    if !stderr.is_empty() {
        if !all.is_empty() && !all.ends_with('\n') {
            all.push('\n');
        }
        if !stdout.is_empty() {
            all.push_str("[stderr]\n");
        }
        all.push_str(stderr);
    }
    if all.len() <= MAX_OUTPUT {
        return all;
    }
    let mut cut = MAX_OUTPUT;
    while !all.is_char_boundary(cut) {
        cut -= 1;
    }
    all.truncate(cut);
    all.push_str(OUTPUT_CUT);
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_only_added() {
        let dir = std::env::temp_dir().join(format!("oso-audit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = Audit::new(dir.join("audit.jsonl"));
        a.log(Entry { pane: 1, action: "run".into(), host: "example".into(), command: "df -h".into(), decision: "allowed".into(), exit_code: Some(0), ..Default::default() });
        a.log(Entry { pane: 1, action: "run".into(), host: "example".into(), command: "reboot".into(), decision: "denied".into(), ..Default::default() });
        let text = std::fs::read_to_string(a.path()).unwrap();
        let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0]["command"].as_str(), lines[0]["decision"].as_str(), lines[0]["exit_code"].as_i64()), (Some("df -h"), Some("allowed"), Some(0)));
        assert_eq!(lines[1]["decision"], "denied");
        assert!(lines[0]["ts"].as_u64().unwrap() > 1_700_000_000, "stamped with the time");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(a.path()).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::remove_dir_all(dir).ok();
    }

    fn line(id: u64, ts: u64, decision: &str) -> Entry {
        Entry { id, ts, pane: 3, agent: "claude-code".into(), action: "run".into(), host: "web".into(), command: "df -h".into(), decision: decision.into(), ..Default::default() }
    }

    #[test]
    fn the_lines_of_one_request_become_one_thing() {
        let mut fin = line(7, 12, "finished");
        fin.exit_code = Some(0);
        fin.preview = "Filesystem".into();
        let mut asked = line(7, 10, "allowed");
        asked.policy = "ask".into();
        asked.reason = "you allowed it once".into();
        let list = activities(&[asked, line(8, 11, "denied"), fin, Entry { action: "context".into(), ..line(9, 13, "allowed_without_asking") }], 20);
        assert_eq!(list.len(), 2, "reading the list of servers is not an activity");
        assert_eq!((list[0].state.as_str(), list[0].decision.as_str(), list[0].exit_code, list[0].preview.as_str()), ("finished", "allowed", Some(0), "Filesystem"));
        assert_eq!((list[0].policy.as_str(), list[0].reason.as_str(), list[0].ts, list[0].updated), ("ask", "you allowed it once", 10, 12));
        assert_eq!((list[1].state.as_str(), list[1].decision.as_str()), ("denied", "denied"));
        // the window reads these names (`Activity` in ui/src/api.ts): they must not drift apart
        let mut keys: Vec<String> = serde_json::to_value(&list[0]).unwrap().as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys.join(","), "action,agent,bytes_out,command,decision,exit_code,has_output,host,id,note,pane,policy,preview,reason,state,ts,updated");
    }

    #[test]
    fn old_lines_without_an_id_are_paired_by_what_they_say() {
        let mut fin = line(0, 12, "finished");
        fin.exit_code = Some(2);
        let other = Entry { command: "uptime".into(), ..line(0, 11, "allowed_without_asking") };
        let list = activities(&[line(0, 10, "allowed"), other, fin], 20);
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].state.as_str(), list[0].exit_code), ("finished", Some(2)));
        assert_eq!(list[1].state, "running", "the other command is still going");
        // one that never reported back is not shown as running for ever
        assert_eq!(activities(&[line(0, 10, "allowed")], 10 + 901)[0].state, "unknown");
    }

    #[test]
    fn the_end_of_the_log_is_read_and_a_cut_first_line_is_skipped() {
        let dir = std::env::temp_dir().join(format!("oso-audit-tail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = Audit::new(dir.join("audit.jsonl"));
        for i in 1..=3000u64 {
            a.log(Entry { id: i, ts: 1_800_000_000 + i, action: "run".into(), host: "h".into(), command: format!("echo {i} {}", "x".repeat(200)), decision: "allowed_without_asking".into(), ..Default::default() });
        }
        let last = recent_activity(a.path(), 5);
        assert_eq!(last.len(), 5);
        assert_eq!(last[4].id, 3000);
        assert!(std::fs::metadata(a.path()).unwrap().len() > 64 * 1024, "the test really cuts a line");
        assert!(recent_tail(a.path(), 100_000, 64 * 1024).iter().all(|e| e.id != 0), "no half line came through");
        assert!(recent_activity(&dir.join("none.jsonl"), 5).is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_preview_keeps_the_first_lines_and_says_when_it_cut() {
        assert_eq!(preview_of("a\nb", ""), "a\nb");
        assert_eq!(preview_of("a", "err"), "a\nerr");
        let long = (1..=20).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
        let p = preview_of(&long, "");
        assert_eq!(p.lines().count(), 8);
        assert!(p.ends_with('…') && p.starts_with("1\n2"));
        assert!(preview_of(&"y".repeat(5000), "").chars().count() <= 801);
    }

    #[test]
    fn the_whole_command_and_output_are_found_by_the_id_of_the_request() {
        let dir = std::env::temp_dir().join(format!("oso-audit-detail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = Audit::new(dir.join("audit.jsonl"));
        let long = format!("echo {}", "x".repeat(3000));
        a.log(Entry { id: 5, ts: 1_800_000_001, action: "run".into(), host: "h".into(), command: long.clone(), decision: "allowed".into(), ..Default::default() });
        a.log(Entry { id: 15, ts: 1_800_000_002, action: "run".into(), host: "h".into(), command: "other".into(), decision: "allowed".into(), ..Default::default() });
        a.log(Entry { id: 5, ts: 1_800_000_003, action: "run".into(), host: "h".into(), command: long.clone(), decision: "finished".into(), output: "line 1\nline 2\n".into(), exit_code: Some(0), ..Default::default() });
        let d = find_detail(a.path(), 5).unwrap();
        assert_eq!((d.command.as_str(), d.output.as_str(), d.cut), (long.as_str(), "line 1\nline 2\n", false));
        assert_eq!(find_detail(a.path(), 15).unwrap().output, "", "a request still running has no output yet");
        assert!(find_detail(a.path(), 1).is_none() && find_detail(a.path(), 0).is_none(), "5 and 15 are not 1");
        assert!(activities(&recent(a.path(), 10), 1_800_000_010).iter().any(|x| x.id == 5 && x.has_output));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn output_is_stdout_then_stderr_and_a_long_one_says_it_was_cut() {
        assert_eq!(output_of("a\n", ""), "a\n");
        assert_eq!(output_of("a", "oops"), "a\n[stderr]\noops");
        assert_eq!(output_of("", "oops"), "oops");
        let big = output_of(&"é".repeat(MAX_OUTPUT), "");
        assert!(big.ends_with(OUTPUT_CUT) && big.len() <= MAX_OUTPUT + OUTPUT_CUT.len());
        assert!(big.is_char_boundary(big.len() - OUTPUT_CUT.len()));
    }
}
