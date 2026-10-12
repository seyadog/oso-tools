//! AI agents that run in a terminal of oso (claude, opencode, codex, or whatever the person lists): which ones run where,
//! and what each is doing. This is what tells you which agent needs you.
//!
//! What an agent is doing comes from the most exact signal there is, in this order (the first that applies wins):
//! 1. `approval`: it has a request waiting in oso → it needs you;
//! 2. `hook`: its own hooks said something (Claude Code's hooks, OpenCode's plugin; see `integration`);
//! 3. `process`: the program ended after having been seen → finished; the program is there → idle.
//! A fourth, a guess from the screen, lives in the window (`ui/src/agentscreen.ts`) and is only used when the first
//! three say nothing more than "idle": the engine never sees what a terminal shows.

use crate::procscan::{self, Proc};
use crate::settings;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// One program that counts as an agent.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AgentKind {
    /// Short name used as the key (`claude`).
    pub id: String,
    /// What the window shows (`Claude Code`).
    pub name: String,
    /// What is typed in a terminal to start it (`claude`, or `claude --model opus`). Its first word is the program that is looked for.
    pub command: String,
}

pub fn default_kinds() -> Vec<AgentKind> {
    [("claude", "Claude Code", "claude"), ("opencode", "OpenCode", "opencode"), ("codex", "Codex", "codex")].iter().map(|(id, name, command)| AgentKind { id: (*id).into(), name: (*name).into(), command: (*command).into() }).collect()
}

/// The list in `settings.json` (key `agents`), or the defaults when there is none (or it is not readable).
pub fn kinds_in(dir: &Path) -> Vec<AgentKind> {
    std::fs::read_to_string(dir.join("settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("agents").cloned())
        .and_then(|a| serde_json::from_value::<Vec<AgentKind>>(a).ok())
        .map(|k| k.into_iter().filter(|k| check(k).is_none()).collect::<Vec<_>>())
        .filter(|k| !k.is_empty())
        .unwrap_or_else(default_kinds)
}

fn check(k: &AgentKind) -> Option<&'static str> {
    if k.id.is_empty() || !k.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Some("an agent's id is letters, digits, - and _");
    }
    if k.name.trim().is_empty() || k.name.len() > 40 {
        return Some("an agent needs a name (up to 40 characters)");
    }
    if k.command.trim().is_empty() || k.command.len() > 300 || k.command.contains('\n') || k.command.contains('\0') {
        return Some("an agent's command is one line");
    }
    None
}

/// Saves the list (an empty one means "back to the defaults").
pub fn save_kinds(dir: &Path, kinds: &[AgentKind]) -> Result<(), String> {
    if kinds.len() > 20 {
        return Err("at most 20 agents".into());
    }
    let mut seen = HashSet::new();
    for k in kinds {
        if let Some(why) = check(k) {
            return Err(why.into());
        }
        if !seen.insert(k.id.clone()) {
            return Err(format!("two agents are called {:?}", k.id));
        }
    }
    let value = if kinds.is_empty() { serde_json::Value::Null } else { serde_json::to_value(kinds).map_err(|e| e.to_string())? };
    settings::save_key_in(dir, "agents", value);
    Ok(())
}

// ------------------------------------------------------------------ detection

/// The program an agent is started with: the first word of its command, without its folder.
fn program(kind: &AgentKind) -> &str {
    let first = kind.command.split_whitespace().next().unwrap_or("");
    first.rsplit('/').next().unwrap_or(first)
}

const INTERPRETERS: [&str; 10] = ["node", "nodejs", "bun", "deno", "python", "python3", "sh", "bash", "env", "npx"];

fn base(arg: &str) -> &str {
    arg.rsplit('/').next().unwrap_or(arg)
}

/// Does this process run the agent? Its own name is the program (a native binary), or it is an interpreter whose first
/// arguments name a path that has the program as a folder or file (`node …/claude-code/cli.js`, `sh -c claude`).
pub fn matches(kind: &AgentKind, argv: &[String]) -> bool {
    let prog = program(kind);
    if prog.is_empty() {
        return false;
    }
    let Some(first) = argv.first() else { return false };
    let name = base(first);
    if name == prog {
        return true;
    }
    if !INTERPRETERS.contains(&name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.')) && !INTERPRETERS.contains(&name) {
        return false;
    }
    argv.iter().skip(1).take(3).filter(|a| !a.starts_with('-') && !a.chars().any(char::is_whitespace)).any(|a| {
        let file = base(a);
        let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
        a.split('/').any(|part| part == prog || part.starts_with(&format!("{prog}-"))) || stem == prog
    })
}

/// An agent found in a terminal.
#[derive(Clone, Debug, PartialEq)]
pub struct Detected {
    pub kind: String,
    pub name: String,
    pub pid: u32,
    pub cwd: PathBuf,
}

/// The agent that runs in a terminal, if any: the process that matches and has no matching child (the one that works).
pub fn detect(kinds: &[AgentKind], procs: &[Proc], proc_root: &Path) -> Option<Detected> {
    let hits: Vec<(&AgentKind, &Proc)> = procs.iter().filter_map(|p| kinds.iter().find(|k| matches(k, &p.argv)).map(|k| (k, p))).collect();
    let leaf = hits.iter().filter(|(_, p)| !hits.iter().any(|(_, c)| c.ppid == p.pid)).max_by_key(|(_, p)| p.pid)?;
    Some(Detected { kind: leaf.0.id.clone(), name: leaf.0.name.clone(), pid: leaf.1.pid, cwd: procscan::cwd_of(proc_root, leaf.1.pid).unwrap_or_default() })
}

/// The branch checked out in `dir` (or the short hash when detached), through worktrees too. `None` outside a repository.
pub fn git_branch(dir: &Path) -> Option<String> {
    let mut at = Some(dir);
    while let Some(d) = at {
        let dot = d.join(".git");
        let git_dir = if dot.is_dir() {
            Some(dot)
        } else if dot.is_file() {
            let text = std::fs::read_to_string(&dot).ok()?;
            let target = text.trim().strip_prefix("gitdir:")?.trim();
            let p = PathBuf::from(target);
            Some(if p.is_absolute() { p } else { d.join(p) })
        } else {
            None
        };
        if let Some(g) = git_dir {
            let head = std::fs::read_to_string(g.join("HEAD")).ok()?;
            let head = head.trim();
            return Some(match head.strip_prefix("ref: refs/heads/") {
                Some(b) => b.to_string(),
                None => head.chars().take(7).collect(),
            });
        }
        at = d.parent();
    }
    None
}

// ------------------------------------------------------------------ state

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum State {
    NeedsYou,
    Working,
    Finished,
    Idle,
}

/// What an agent's own hook said, as a state. `None`: nothing to learn from it.
pub fn state_for(agent: &str, event: &str) -> Option<State> {
    match (agent, event) {
        ("claude", "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" | "SubagentStart") => Some(State::Working),
        ("claude", "PermissionRequest" | "Notification" | "Elicitation") => Some(State::NeedsYou),
        ("claude", "Stop" | "StopFailure" | "SessionEnd") => Some(State::Finished),
        ("claude", "SessionStart") => Some(State::Idle),
        ("opencode", "permission.asked") => Some(State::NeedsYou),
        ("opencode", "permission.replied" | "session.busy" | "session.working") => Some(State::Working),
        ("opencode", "session.idle" | "session.error") => Some(State::Finished),
        ("opencode", "session.created") => Some(State::Idle),
        _ => None,
    }
}

/// One agent, as the window lists it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Status {
    pub sid: u64,
    pub kind: String,
    pub name: String,
    pub pid: Option<u32>,
    pub cwd: String,
    /// The last part of the folder.
    pub folder: String,
    pub branch: String,
    pub state: State,
    /// When it got into this state (ms since 1970).
    pub since: u64,
    /// `approval`, `hook` or `process`: how the state is known.
    pub source: &'static str,
}

#[derive(Default)]
struct Mem {
    kind: String,
    name: String,
    pid: Option<u32>,
    cwd: PathBuf,
    /// An agent process was seen in this terminal.
    seen: bool,
    /// It was seen, and is not there any more.
    gone: bool,
    /// The person looked at it since it finished: it is not listed any more.
    dismissed: bool,
    hook: Option<(State, u64)>,
    state: Option<State>,
    since: u64,
}

/// Remembers, per terminal, what was seen and heard, and works out the state of each agent.
#[derive(Default)]
pub struct Tracker {
    per: HashMap<u64, Mem>,
}

impl Tracker {
    /// An agent's hook reported something (the terminal is known from the token, not from the agent).
    pub fn event(&mut self, sid: u64, agent: &str, state: State, now: u64, kinds: &[AgentKind]) {
        let m = self.per.entry(sid).or_default();
        m.hook = Some((state, now));
        m.dismissed = false;
        if m.kind.is_empty() {
            m.kind = agent.to_string();
            m.name = kinds.iter().find(|k| k.id == agent).map(|k| k.name.clone()).unwrap_or_else(|| agent.to_string());
        }
    }

    /// The person looked at this terminal: "finished" calms down to "idle", and a program that ended is not listed any more.
    pub fn ack(&mut self, sid: u64, now: u64) {
        if let Some(m) = self.per.get_mut(&sid) {
            if m.gone {
                m.dismissed = true;
            } else if matches!(m.hook, Some((State::Finished, _))) {
                m.hook = Some((State::Idle, now));
            }
        }
    }

    /// Works out every agent. `found`: what was detected per terminal; `pending`: terminals with a request waiting in oso;
    /// `live`: the terminals that exist (the memory of a closed one is dropped).
    pub fn update(&mut self, now: u64, found: &HashMap<u64, Detected>, pending: &[u64], live: &HashSet<u64>) -> Vec<Status> {
        self.per.retain(|sid, _| live.contains(sid));
        for sid in pending.iter().chain(found.keys()).filter(|s| live.contains(s)) {
            self.per.entry(*sid).or_default();
        }
        let mut out = vec![];
        for (&sid, m) in self.per.iter_mut() {
            if let Some(d) = found.get(&sid) {
                if m.gone || m.dismissed {
                    // a new run in the same terminal: nothing of the old one counts
                    m.hook = None;
                    m.gone = false;
                    m.dismissed = false;
                }
                (m.kind, m.name, m.pid, m.cwd, m.seen) = (d.kind.clone(), d.name.clone(), Some(d.pid), d.cwd.clone(), true);
            } else if m.seen && !m.gone {
                m.gone = true;
                m.hook = None;
                m.pid = None;
            }
            let waiting = pending.contains(&sid);
            let known = m.seen || m.hook.is_some() || waiting;
            if !known || (m.dismissed && !waiting) {
                continue;
            }
            let (state, source) = if waiting {
                (State::NeedsYou, "approval")
            } else if m.gone {
                (State::Finished, "process")
            } else if let Some((s, _)) = m.hook {
                (s, "hook")
            } else {
                (State::Idle, "process")
            };
            if m.state != Some(state) {
                m.state = Some(state);
                m.since = now;
            }
            let cwd = m.cwd.to_string_lossy().into_owned();
            let folder = m.cwd.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let branch = if m.cwd.as_os_str().is_empty() { String::new() } else { git_branch(&m.cwd).unwrap_or_default() };
            let kind = if m.kind.is_empty() { "agent".to_string() } else { m.kind.clone() };
            let name = if m.name.is_empty() { "AI agent".to_string() } else { m.name.clone() };
            out.push(Status { sid, kind, name, pid: m.pid, cwd, folder, branch, state, since: m.since, source });
        }
        out.sort_by_key(|s| s.sid);
        out
    }
}

// ------------------------------------------------------------------ worktrees

/// A folder-safe version of a branch name (`oso/claude-241011-1430` → `oso-claude-241011-1430`).
pub fn slug_of(branch: &str) -> String {
    let s: String = branch.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '-' }).collect();
    s.trim_matches(|c| c == '-' || c == '.').to_string()
}

/// Is this a branch name git accepts and that cannot be taken for an option?
pub fn branch_ok(branch: &str) -> bool {
    !branch.is_empty() && branch.len() <= 120 && !branch.starts_with('-') && !branch.chars().any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c)) && !branch.contains("..") && !branch.contains("@{") && !branch.ends_with('/') && !branch.ends_with(".lock") && !branch.contains("//")
}

/// `git worktree add -b <branch> <repo>.worktrees/<slug>`: a new branch in its own folder next to the repository, so that
/// agents do not step on each other and the repository gets no untracked files. Returns the folder.
pub fn worktree_add(repo: &Path, branch: &str) -> Result<PathBuf, String> {
    if !branch_ok(branch) {
        return Err(format!("{branch:?} is not a branch name git accepts"));
    }
    if !repo.is_dir() {
        return Err(format!("{} is not a folder", repo.display()));
    }
    let top = std::process::Command::new("git").arg("-C").arg(repo).args(["rev-parse", "--show-toplevel"]).output().map_err(|e| format!("could not run git: {e}"))?;
    if !top.status.success() {
        return Err(format!("{} is not inside a git repository", repo.display()));
    }
    let top = PathBuf::from(String::from_utf8_lossy(&top.stdout).trim());
    let name = top.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into());
    let dir = top.with_file_name(format!("{name}.worktrees")).join(slug_of(branch));
    if dir.exists() {
        return Err(format!("{} already exists", dir.display()));
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("could not make {}: {e}", parent.display()))?;
    }
    let out = std::process::Command::new("git").arg("-C").arg(&top).args(["worktree", "add", "-b", branch]).arg(&dir).output().map_err(|e| format!("could not run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().lines().last().unwrap_or("git could not make the worktree").to_string());
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oso-agents-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn argv(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_string).collect()
    }

    fn kinds() -> Vec<AgentKind> {
        default_kinds()
    }

    #[test]
    fn an_agent_is_recognised_by_its_program_also_behind_an_interpreter() {
        let claude = &kinds()[0];
        for a in ["claude", "/home/u/.local/bin/claude --resume", "node /usr/lib/node_modules/@anthropic-ai/claude-code/cli.js", "bun /x/claude/main.ts", "sh -c claude", "env claude", "node cli/claude.js"] {
            assert!(matches(claude, &argv(a)), "{a}");
        }
        for a in ["vim claude/notes.md", "bash", "cat /home/u/.claude/settings.json", "grep claude file", "node server.js", "claudex", "less /srv/claude/log"] {
            assert!(!matches(claude, &argv(a)), "{a}");
        }
        // a long argument with spaces (a script given to `bash -c`) is not a path to an agent
        assert!(!matches(claude, &["/bin/bash".into(), "-c".into(), "source /home/u/.claude/shell-snapshots/x && eval claude".into()]));
        // the program of a custom agent is the first word of its command
        let mine = AgentKind { id: "aider".into(), name: "Aider".into(), command: "/opt/bin/aider --model x".into() };
        assert!(matches(&mine, &argv("aider --model x")) && matches(&mine, &argv("python3 /usr/bin/aider")));
    }

    #[test]
    fn the_agent_that_works_is_the_one_without_a_child_that_is_one_too() {
        let procs = vec![Proc { pid: 10, ppid: 1, argv: argv("sh -c claude") }, Proc { pid: 11, ppid: 10, argv: argv("claude") }, Proc { pid: 12, ppid: 11, argv: argv("bash") }, Proc { pid: 20, ppid: 1, argv: argv("vim x") }];
        let d = detect(&kinds(), &procs, Path::new("/proc")).unwrap();
        assert_eq!((d.kind.as_str(), d.pid), ("claude", 11));
        assert!(detect(&kinds(), &procs[3..], Path::new("/proc")).is_none());
    }

    #[test]
    fn a_real_program_is_found_by_its_terminal_and_its_folder_and_branch_are_read() {
        let repo = temp("branch");
        assert!(Command::new("git").arg("-C").arg(&repo).args(["init", "-q", "-b", "feature/x"]).status().unwrap().success());
        let mut child = Command::new("bash").args(["-c", "exec -a claude sleep 30"]).current_dir(&repo).env("OSO_PANE", "9191").stdout(Stdio::null()).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let found = procscan::scan_panes();
        let d = detect(&kinds(), &found[&9191], Path::new("/proc")).expect("claude is found");
        assert_eq!((d.kind.as_str(), d.pid), ("claude", child.id()));
        assert_eq!(d.cwd.canonicalize().unwrap(), repo.canonicalize().unwrap());
        assert_eq!(git_branch(&d.cwd).as_deref(), Some("feature/x"));
        let mut t = Tracker::default();
        let live = HashSet::from([9191]);
        let s = t.update(1000, &HashMap::from([(9191, d)]), &[], &live);
        assert_eq!((s[0].state, s[0].source, s[0].folder.as_str(), s[0].branch.as_str()), (State::Idle, "process", repo.file_name().unwrap().to_str().unwrap(), "feature/x"));
        let _ = child.kill();
        let _ = child.wait();
        std::fs::remove_dir_all(repo).ok();
    }

    fn found(sid: u64, kind: &str) -> HashMap<u64, Detected> {
        HashMap::from([(sid, Detected { kind: kind.into(), name: kind.to_uppercase(), pid: 7, cwd: PathBuf::from("/srv/acme") })])
    }

    fn state_of(t: &mut Tracker, now: u64, found: &HashMap<u64, Detected>, pending: &[u64]) -> (State, &'static str, u64) {
        let live = HashSet::from([1, 2, 3]);
        let s = t.update(now, found, pending, &live);
        let s = s.iter().find(|s| s.sid == 1).expect("listed");
        (s.state, s.source, s.since)
    }

    #[test]
    fn exact_signals_come_first_a_request_then_the_hooks_then_the_process() {
        let mut t = Tracker::default();
        let f = found(1, "claude");
        assert_eq!(state_of(&mut t, 100, &f, &[]), (State::Idle, "process", 100), "only the process is known");
        t.event(1, "claude", State::Working, 200, &kinds());
        assert_eq!(state_of(&mut t, 210, &f, &[]), (State::Working, "hook", 210));
        assert_eq!(state_of(&mut t, 300, &f, &[1]), (State::NeedsYou, "approval", 300), "a request waiting beats what the hook said");
        assert_eq!(state_of(&mut t, 310, &f, &[]), (State::Working, "hook", 310), "and it goes back when it is answered");
        t.event(1, "claude", State::NeedsYou, 400, &kinds());
        assert_eq!(state_of(&mut t, 410, &f, &[]), (State::NeedsYou, "hook", 410), "its own notification also says it needs you");
        t.event(1, "claude", State::Finished, 500, &kinds());
        assert_eq!(state_of(&mut t, 510, &f, &[]), (State::Finished, "hook", 510));
        assert_eq!(state_of(&mut t, 520, &f, &[]).2, 510, "since does not move while the state stays");
        t.ack(1, 600);
        assert_eq!(state_of(&mut t, 610, &f, &[]), (State::Idle, "hook", 610), "looking at it calms finished down to idle");
    }

    #[test]
    fn a_program_that_ends_is_finished_until_it_is_looked_at_and_then_is_not_listed() {
        let mut t = Tracker::default();
        let f = found(1, "opencode");
        t.event(1, "opencode", State::Working, 50, &kinds());
        assert_eq!(state_of(&mut t, 100, &f, &[]).0, State::Working);
        let none = HashMap::new();
        assert_eq!(state_of(&mut t, 200, &none, &[]), (State::Finished, "process", 200), "the process ended: finished, and the old hook means nothing now");
        t.ack(1, 300);
        let live = HashSet::from([1]);
        assert!(t.update(310, &none, &[], &live).is_empty(), "looked at: it is not there any more");
        // a new run in the same terminal starts clean
        let s = t.update(400, &found(1, "opencode"), &[], &live);
        assert_eq!((s[0].state, s[0].source), (State::Idle, "process"));
    }

    #[test]
    fn a_request_from_a_terminal_without_a_known_agent_is_still_listed_and_a_closed_terminal_is_forgotten() {
        let mut t = Tracker::default();
        let live = HashSet::from([1, 2]);
        let s = t.update(10, &HashMap::new(), &[2], &live);
        assert_eq!((s.len(), s[0].sid, s[0].state, s[0].name.as_str()), (1, 2, State::NeedsYou, "AI agent"));
        assert!(t.update(20, &HashMap::new(), &[], &live).is_empty(), "answered, and nothing else is known about it");
        t.event(1, "claude", State::Working, 30, &kinds());
        let listed = t.update(40, &HashMap::new(), &[], &live);
        assert_eq!((listed.len(), listed[0].name.as_str()), (1, "Claude Code"), "a hook alone is enough to list an agent");
        assert!(t.update(50, &HashMap::new(), &[], &HashSet::from([2])).is_empty(), "terminal 1 closed");
        assert!(t.per.get(&1).is_none());
    }

    #[test]
    fn the_hooks_of_each_agent_map_to_states_and_unknown_events_say_nothing() {
        assert_eq!(state_for("claude", "UserPromptSubmit"), Some(State::Working));
        assert_eq!(state_for("claude", "PermissionRequest"), Some(State::NeedsYou));
        assert_eq!(state_for("claude", "Notification"), Some(State::NeedsYou));
        assert_eq!(state_for("claude", "Stop"), Some(State::Finished));
        assert_eq!(state_for("opencode", "permission.asked"), Some(State::NeedsYou));
        assert_eq!(state_for("opencode", "session.idle"), Some(State::Finished));
        assert_eq!(state_for("opencode", "Stop"), None, "a Claude event means nothing for OpenCode");
        assert_eq!(state_for("codex", "Stop"), None);
        assert_eq!(state_for("claude", "PreCompact"), None);
    }

    #[test]
    fn the_list_of_agents_is_edited_in_settings_and_a_bad_one_is_refused() {
        let dir = temp("kinds");
        assert_eq!(kinds_in(&dir), default_kinds());
        let mut mine = default_kinds();
        mine.push(AgentKind { id: "aider".into(), name: "Aider".into(), command: "aider --model sonnet".into() });
        save_kinds(&dir, &mine).unwrap();
        assert_eq!(kinds_in(&dir).len(), 4);
        // the rest of settings.json is kept
        crate::settings::save_key_in(&dir, "theme", serde_json::json!("classic"));
        assert_eq!(kinds_in(&dir).len(), 4);
        assert!(save_kinds(&dir, &[AgentKind { id: "a b".into(), name: "x".into(), command: "x".into() }]).is_err());
        assert!(save_kinds(&dir, &[AgentKind { id: "a".into(), name: "x".into(), command: "x\ny".into() }]).is_err());
        assert!(save_kinds(&dir, &[mine[0].clone(), mine[0].clone()]).is_err(), "two with the same id");
        save_kinds(&dir, &[]).unwrap();
        assert_eq!(kinds_in(&dir), default_kinds(), "an empty list is the defaults again");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_branch_name_is_checked_before_it_reaches_git() {
        for b in ["oso/claude-241011-1430", "feature/x", "fix_1.2"] {
            assert!(branch_ok(b), "{b}");
        }
        for b in ["", "-D", "a b", "a..b", "a~1", "x/", "a//b", "a.lock", "a:b", "--force"] {
            assert!(!branch_ok(b), "{b}");
        }
        assert_eq!(slug_of("oso/claude-241011-1430"), "oso-claude-241011-1430");
    }

    #[test]
    fn a_worktree_is_a_new_branch_in_a_folder_next_to_the_repository() {
        let base = temp("worktree");
        let repo = base.join("acme");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| Command::new("git").arg("-C").arg(&repo).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap();
        assert!(git(&["init", "-q", "-b", "main"]).status.success());
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        git(&["add", "."]);
        assert!(git(&["commit", "-q", "-m", "first"]).status.success());
        let dir = worktree_add(&repo, "oso/claude-1").unwrap();
        assert_eq!(dir, base.join("acme.worktrees").join("oso-claude-1"));
        assert!(dir.join("a.txt").exists(), "the files are there");
        assert_eq!(git_branch(&dir).as_deref(), Some("oso/claude-1"), "its own branch, read through the worktree");
        assert_eq!(git_branch(&repo).as_deref(), Some("main"), "the repository is where it was");
        assert!(!repo.join("acme.worktrees").exists() && git(&["status", "--porcelain"]).stdout.is_empty(), "the repository gets no untracked files");
        assert!(worktree_add(&repo, "oso/claude-1").unwrap_err().contains("already exists"));
        assert!(worktree_add(&repo, "-D").is_err());
        assert!(worktree_add(&base, "x").unwrap_err().contains("not inside a git repository"));
        assert!(worktree_add(&base.join("nope"), "x").is_err());
        std::fs::remove_dir_all(base).ok();
    }
}
