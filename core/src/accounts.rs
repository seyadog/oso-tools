//! The accounts of other tools that oso only *switches*: GitHub (`gh`) and Tailscale. oso never keeps their tokens;
//! it reads who is active, and asks the tool itself to change. Every call to a program goes through a `Runner`, so
//! the tests give it canned answers instead of running the real binaries.

use serde::Serialize;
use std::process::{Command, Stdio};
use std::time::Duration;

/// What a program printed.
pub struct Out {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs a program. `None`: it is not installed (or did not start).
pub trait Runner: Send {
    fn run(&self, program: &str, args: &[&str]) -> Option<Out>;
}

/// The real thing: it gives up on a program that does not answer in 8 seconds.
pub struct RealRunner;

impl Runner for RealRunner {
    fn run(&self, program: &str, args: &[&str]) -> Option<Out> {
        let child = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        let out = rx.recv_timeout(Duration::from_secs(8)).ok()?.ok()?;
        Some(Out { ok: out.status.success(), stdout: String::from_utf8_lossy(&out.stdout).into_owned(), stderr: String::from_utf8_lossy(&out.stderr).into_owned() })
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Account {
    /// What to give back to switch to it.
    pub id: String,
    /// What a person calls it (a user, or a tailnet).
    pub name: String,
    /// Where it is (host, tailnet…).
    pub detail: String,
    pub active: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct ToolAccounts {
    pub installed: bool,
    pub accounts: Vec<Account>,
    /// Why the list may be incomplete, in words.
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct AccountsView {
    pub github: ToolAccounts,
    pub tailscale: ToolAccounts,
}

// ------------------------------------------------------------------ GitHub

/// `gh auth status --json hosts`: every account of every host. The id is `host/login`.
pub fn parse_gh(json: &str) -> Vec<Account> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return vec![] };
    let mut out = vec![];
    let Some(hosts) = v.get("hosts").and_then(|h| h.as_object()) else { return out };
    for (host, list) in hosts {
        for a in list.as_array().into_iter().flatten() {
            let Some(login) = a.get("login").and_then(|l| l.as_str()) else { continue };
            let state = a.get("state").and_then(|s| s.as_str()).unwrap_or("");
            out.push(Account {
                id: format!("{host}/{login}"),
                name: login.to_string(),
                detail: if state == "success" || state.is_empty() { host.clone() } else { format!("{host} (needs login again)") },
                active: a.get("active").and_then(|x| x.as_bool()).unwrap_or(false),
            });
        }
    }
    out
}

pub fn github(r: &dyn Runner) -> ToolAccounts {
    let Some(out) = r.run("gh", &["auth", "status", "--json", "hosts"]) else { return ToolAccounts::default() };
    let accounts = parse_gh(&out.stdout);
    let error = (accounts.is_empty() && !out.stderr.trim().is_empty()).then(|| "GitHub: no account is logged in (run `gh auth login`)".to_string());
    ToolAccounts { installed: true, accounts, error }
}

pub fn github_switch(r: &dyn Runner, id: &str) -> Result<(), String> {
    let (host, user) = id.split_once('/').unwrap_or(("github.com", id));
    if user.is_empty() || user.starts_with('-') || host.starts_with('-') {
        return Err("that is not a GitHub account".into());
    }
    let out = r.run("gh", &["auth", "switch", "--hostname", host, "--user", user]).ok_or("`gh` is not installed")?;
    if out.ok { Ok(()) } else { Err(format!("GitHub: {}", first_line(&out.stderr, "could not switch the account"))) }
}

// ------------------------------------------------------------------ Tailscale

/// `tailscale switch --list`: a header (`ID  Tailnet  Account`), then one account per line, the active one marked `*`.
pub fn parse_tailscale_list(text: &str) -> Vec<Account> {
    let mut out = vec![];
    // only what follows the header is an account: an error message has no header
    for line in text.lines().skip_while(|l| !l.trim_start().starts_with("ID")).skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let active = line.split_whitespace().any(|w| w == "*") || line.ends_with('*');
        let cols: Vec<&str> = line.split_whitespace().filter(|w| *w != "*").map(|w| w.trim_end_matches('*')).filter(|w| !w.is_empty()).collect();
        if cols.len() < 2 {
            continue;
        }
        let (id, tailnet, account) = (cols[0], cols[1], cols.get(2).copied().unwrap_or(""));
        out.push(Account { id: id.into(), name: if account.is_empty() { tailnet.into() } else { account.into() }, detail: tailnet.into(), active });
    }
    out
}

/// The tailnet the machine is on now, from `tailscale status --json`.
pub fn parse_tailscale_current(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get("CurrentTailnet")?.get("Name")?.as_str().map(str::to_string)
}

pub fn tailscale(r: &dyn Runner) -> ToolAccounts {
    let Some(status) = r.run("tailscale", &["status", "--json"]) else { return ToolAccounts::default() };
    let current = parse_tailscale_current(&status.stdout);
    let list = r.run("tailscale", &["switch", "--list"]);
    let mut accounts = list.as_ref().filter(|l| l.ok).map(|l| parse_tailscale_list(&l.stdout)).unwrap_or_default();
    let mut error = None;
    if accounts.is_empty() {
        error = Some(match &list {
            Some(l) if l.stderr.to_lowercase().contains("access denied") || l.stdout.to_lowercase().contains("access denied") => {
                "Tailscale only lists its accounts to an operator: run `sudo tailscale set --operator=$USER` once".to_string()
            }
            _ => "Tailscale did not list its accounts".to_string(),
        });
        // what is known without the list: the one in use
        if let Some(name) = &current {
            accounts.push(Account { id: name.clone(), name: name.clone(), detail: "in use now".into(), active: true });
        }
    }
    ToolAccounts { installed: true, accounts, error }
}

pub fn tailscale_switch(r: &dyn Runner, id: &str) -> Result<(), String> {
    if id.is_empty() || id.starts_with('-') {
        return Err("that is not a Tailscale account".into());
    }
    let out = r.run("tailscale", &["switch", id]).ok_or("`tailscale` is not installed")?;
    if out.ok { Ok(()) } else { Err(format!("Tailscale: {}", first_line(&out.stderr, "could not switch the account"))) }
}

/// Does `wanted` (an id, a tailnet, an account or a display name) name the account that is active now?
pub fn is_active(view: &ToolAccounts, wanted: &str) -> bool {
    let w = wanted.trim().to_lowercase();
    view.accounts.iter().any(|a| a.active && [&a.id, &a.name, &a.detail].iter().any(|x| x.to_lowercase() == w))
}

pub fn active_name(view: &ToolAccounts) -> Option<String> {
    view.accounts.iter().find(|a| a.active).map(|a| a.name.clone())
}

fn first_line(text: &str, fallback: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.chars().take(200).collect()).unwrap_or_else(|| fallback.to_string())
}

/// Canned answers for tests (here and in the engine's).
#[cfg(test)]
pub mod testing {
    use super::{Out, Runner};
    use std::sync::Mutex;

    /// Answers by program and first words (`None`: not installed); remembers every call.
    pub struct Fake {
        pub answers: Vec<(&'static str, Option<(bool, &'static str, &'static str)>)>,
        pub calls: std::sync::Arc<Mutex<Vec<String>>>,
    }

    impl Runner for Fake {
        fn run(&self, program: &str, args: &[&str]) -> Option<Out> {
            let line = format!("{program} {}", args.join(" "));
            self.calls.lock().unwrap().push(line.clone());
            let (_, a) = self.answers.iter().find(|(k, _)| line.starts_with(k))?;
            let (ok, stdout, stderr) = (*a)?;
            Some(Out { ok, stdout: stdout.into(), stderr: stderr.into() })
        }
    }

    pub const TS_LIST: &str = "ID    Tailnet          Account\n4ab1  corp.example.com  me@corp.example.com  *\n9fe2  home.ts.net      me@gmail.com\n";
    pub const TS_STATUS: &str = r#"{"Version":"1.102.4","BackendState":"Running","CurrentTailnet":{"Name":"corp.example.com","MagicDNSSuffix":"corp.ts.net","MagicDNSEnabled":true}}"#;
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    const GH: &str = r#"{"hosts":{"github.com":[{"state":"success","active":false,"host":"github.com","login":"work-user","tokenSource":"keyring","scopes":"repo","gitProtocol":"https"},{"state":"success","active":true,"host":"github.com","login":"me","tokenSource":"keyring","scopes":"repo","gitProtocol":"https"}],"git.corp.example":[{"state":"error","active":true,"host":"git.corp.example","login":"corp","tokenSource":"keyring","scopes":"","gitProtocol":"ssh"}]}}"#;

    #[test]
    fn github_accounts_are_read_with_the_active_one() {
        let a = parse_gh(GH);
        assert_eq!(a.len(), 3);
        let me = a.iter().find(|x| x.name == "me").unwrap();
        assert!(me.active && me.id == "github.com/me");
        assert!(!a.iter().find(|x| x.name == "work-user").unwrap().active);
        assert!(a.iter().find(|x| x.name == "corp").unwrap().detail.contains("needs login again"));
        assert!(parse_gh("not json").is_empty() && parse_gh("{}").is_empty());
        assert!(!GH.contains("ghp_"), "no token is ever part of what is read");
    }

    #[test]
    fn tailscale_accounts_are_read_from_the_list_and_from_the_status() {
        let a = parse_tailscale_list(TS_LIST);
        assert_eq!(a.len(), 2);
        assert_eq!((a[0].id.as_str(), a[0].detail.as_str(), a[0].name.as_str(), a[0].active), ("4ab1", "corp.example.com", "me@corp.example.com", true));
        assert!(!a[1].active);
        assert_eq!(parse_tailscale_current(TS_STATUS).as_deref(), Some("corp.example.com"));
        assert_eq!(parse_tailscale_current("{}"), None);
        // the "access denied" text of a machine where the user is not an operator is not an account
        assert!(parse_tailscale_list("Access denied: profiles access denied\n\nUse 'sudo tailscale switch --list --json'.\n").is_empty());
    }

    fn fake(answers: Vec<(&'static str, Option<(bool, &'static str, &'static str)>)>) -> Fake {
        Fake { answers, calls: Default::default() }
    }

    #[test]
    fn a_tool_that_is_not_installed_is_hidden() {
        let r = fake(vec![("gh", None), ("tailscale", None)]);
        assert_eq!(github(&r), ToolAccounts::default());
        assert_eq!(tailscale(&r), ToolAccounts::default());
        assert!(!github(&r).installed);
    }

    #[test]
    fn without_operator_rights_tailscale_still_says_which_account_is_in_use() {
        let r = fake(vec![("tailscale status", Some((true, TS_STATUS, ""))), ("tailscale switch --list", Some((false, "", "Access denied: profiles access denied\n\nUse 'sudo tailscale switch --list --json'.")))]);
        let t = tailscale(&r);
        assert!(t.installed && t.accounts.len() == 1 && t.accounts[0].active);
        assert!(t.error.as_deref().unwrap().contains("--operator"), "{:?}", t.error);
        assert!(is_active(&t, "corp.example.com"));
    }

    #[test]
    fn the_wanted_account_can_be_named_in_any_of_its_names() {
        let r = fake(vec![("tailscale status", Some((true, TS_STATUS, ""))), ("tailscale switch --list", Some((true, TS_LIST, "")))]);
        let t = tailscale(&r);
        for name in ["4ab1", "corp.example.com", "me@corp.example.com", "ME@corp.example.com"] {
            assert!(is_active(&t, name), "{name}");
        }
        assert!(!is_active(&t, "home.ts.net") && !is_active(&t, "me@gmail.com"));
        assert_eq!(active_name(&t).as_deref(), Some("me@corp.example.com"));
    }

    #[test]
    fn switching_calls_the_tool_with_the_right_words_and_says_why_when_it_fails() {
        let r = fake(vec![("gh auth switch", Some((true, "", ""))), ("tailscale switch", Some((false, "", "\nno such account\n")))]);
        github_switch(&r, "github.com/work-user").unwrap();
        assert_eq!(r.calls.lock().unwrap()[0], "gh auth switch --hostname github.com --user work-user");
        assert_eq!(tailscale_switch(&r, "home.ts.net").unwrap_err(), "Tailscale: no such account");
        assert!(github_switch(&r, "-oBad").is_err() && tailscale_switch(&r, "--x").is_err() && tailscale_switch(&r, "").is_err());
        let none = fake(vec![("gh", None)]);
        assert!(github_switch(&none, "me").unwrap_err().contains("not installed"));
    }
}
