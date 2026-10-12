//! oso's command line against a real window-side engine: unlock, Spaces, `oso run`, hosts, scanning `.env` files.
//! No command prints a secret.

use oso_core::approvals::{Approvals, Decision};
use oso_core::cli::{self, Term};
use oso_core::control::{self, Api};
use oso_core::engine::{Emit, Engine};
use oso_core::store::Store;
use oso_core::vault::{SecretKind, SecretMeta, Vault};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const TRAP_VISIBLE: &str = "TRAP-VISIBLE-aa11bb22";
const TRAP_HIDDEN: &str = "TRAP-HIDDEN-cc33dd44";
const TRAP_OTHER: &str = "TRAP-OTHER-ee55ff66";

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("oso-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A terminal played by the test: what is typed, what is answered, what is printed and what would be run.
#[derive(Default)]
struct Fake {
    password: String,
    answers: Vec<bool>,
    out: Vec<String>,
    err: Vec<String>,
    ran: Option<(String, Vec<String>, Vec<(String, String)>)>,
}

impl Term for Fake {
    fn password(&mut self, _prompt: &str) -> String {
        self.password.clone()
    }
    fn confirm(&mut self, q: &str) -> bool {
        self.out.push(format!("? {q}"));
        if self.answers.is_empty() { false } else { self.answers.remove(0) }
    }
    fn out(&mut self, text: &str) {
        self.out.push(text.into());
    }
    fn err(&mut self, text: &str) {
        self.err.push(text.into());
    }
    fn exec(&mut self, program: &str, args: &[String], env: &[(String, String)]) -> i32 {
        self.ran = Some((program.into(), args.to_vec(), env.to_vec()));
        0
    }
}

impl Fake {
    fn said(&self) -> String {
        format!("{}\n{}", self.out.join("\n"), self.err.join("\n"))
    }
}

struct Rig {
    engine: Arc<Mutex<Engine>>,
    approvals: Arc<Approvals>,
    socket: PathBuf,
    dir: PathBuf,
    events: Arc<Mutex<Vec<(String, Value)>>>,
    _control: control::Control,
}

fn rig(name: &str) -> Rig {
    let dir = temp(name);
    let emit: Emit = Arc::new(|_, _| {});
    let store = Store::load_from(dir.join("servers.json")).unwrap();
    let mut e = Engine::new(store, Vault::new(dir.join("vault.json")), emit);
    e.vault_unlock("master").unwrap();
    let meta = |name: &str, var: &str, visible| SecretMeta { id: 0, name: name.into(), kind: SecretKind::ApiKey, env_var: Some(var.into()), agent_visible: visible, notes: String::new(), updated: 0 };
    // one secret in General (in no Space), two in the Space Work
    e.secret_save(meta("visible", "VISIBLE_KEY", true), Some(TRAP_VISIBLE)).unwrap();
    let hidden = e.secret_save(meta("hidden", "HIDDEN_KEY", false), Some(TRAP_HIDDEN)).unwrap();
    let work_key = e.secret_save(meta("work key", "WORK_KEY", true), Some(TRAP_OTHER)).unwrap();
    e.space_save(&oso_core::spaces::Space { name: "Work".into(), secrets: vec![hidden, work_key], ..Default::default() }).unwrap();
    e.vault_lock();
    let engine = Arc::new(Mutex::new(e));
    let approvals = Approvals::new();
    let events: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
    let ev = Arc::clone(&events);
    let api = Arc::new(Api { engine: Arc::clone(&engine), approvals: Arc::clone(&approvals), ui: Arc::new(move |k, v| ev.lock().unwrap().push((k.to_string(), v))) });
    let socket = dir.join("control.sock");
    let c = control::start(socket.clone(), Arc::new(move |req| api.handle(req))).unwrap();
    Rig { engine, approvals, socket, dir, events, _control: c }
}

fn run(r: &Rig, t: &mut Fake, args: &[&str]) -> i32 {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    cli::run(&args, &r.socket, t)
}

fn person(a: &Arc<Approvals>, d: Decision) -> Arc<Mutex<Vec<String>>> {
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let (s, a) = (Arc::clone(&seen), Arc::clone(a));
    a.on_request(Arc::new({
        let a = Arc::clone(&a);
        move |r| {
            s.lock().unwrap().push(r.command.clone());
            let a = Arc::clone(&a);
            std::thread::spawn(move || {
                a.answer(r.id, d);
            });
        }
    }));
    seen
}

#[test]
fn unlock_lock_and_status_go_through_the_window_and_the_password_is_asked_not_shown() {
    let r = rig("unlock");
    let mut t = Fake::default();
    assert_eq!(run(&r, &mut t, &["status"]), 0);
    assert!(t.said().contains("vault: locked") && t.said().contains("Space: General"), "{}", t.said());
    let mut t = Fake { password: "wrong".into(), ..Default::default() };
    assert_eq!(run(&r, &mut t, &["unlock"]), 1);
    assert!(t.said().contains("Wrong master password"), "{}", t.said());
    let mut t = Fake { password: "master".into(), ..Default::default() };
    assert_eq!(run(&r, &mut t, &["unlock"]), 0);
    assert!(r.engine.lock().unwrap().vault.is_unlocked());
    assert!(!t.said().contains("master"), "the password is never echoed");
    let mut t = Fake::default();
    run(&r, &mut t, &["status"]);
    assert!(t.said().contains("vault: unlocked"));
    assert_eq!(run(&r, &mut t, &["lock"]), 0);
    assert!(!r.engine.lock().unwrap().vault.is_unlocked());
    assert!(r.events.lock().unwrap().iter().any(|(k, _)| k == "refresh"), "the window is told to look again");
    let mut t = Fake { password: String::new(), ..Default::default() };
    assert_eq!(run(&r, &mut t, &["unlock"]), 1, "an empty password is not sent");
}

#[test]
fn without_a_window_the_command_line_says_so() {
    let mut t = Fake::default();
    let nowhere = std::env::temp_dir().join("oso-no-such-control.sock");
    assert_eq!(cli::run(&["status".to_string()], &nowhere, &mut t), 1);
    assert!(t.said().contains("oso is not open"), "{}", t.said());
}

#[test]
fn spaces_are_listed_and_switched() {
    let r = rig("space");
    let mut t = Fake::default();
    assert_eq!(run(&r, &mut t, &["space"]), 0);
    assert!(t.out.iter().any(|l| l.starts_with("* General")) && t.out.iter().any(|l| l.trim() == "Work"), "{:?}", t.out);
    let mut t = Fake::default();
    assert_eq!(run(&r, &mut t, &["space", "use", "work"]), 0, "by name, any case");
    let work = r.engine.lock().unwrap().spaces_list()[0].id;
    assert_eq!(r.engine.lock().unwrap().active_space(), Some(work));
    assert!(t.said().contains("Space: Work"));
    assert_eq!(run(&r, &mut Fake::default(), &["space", "use", "General"]), 0);
    assert_eq!(r.engine.lock().unwrap().active_space(), None);
    assert_eq!(run(&r, &mut Fake::default(), &["space", "use", "nope"]), 1);
}

#[test]
fn run_hands_the_visible_secrets_of_general_without_asking_and_never_prints_them() {
    let r = rig("run-visible");
    r.engine.lock().unwrap().vault_unlock("master").unwrap();
    let mut t = Fake::default();
    assert_eq!(run(&r, &mut t, &["run", "--", "env"]), 0, "nothing new is handed over, so nobody is asked: {}", t.said());
    let (prog, args, env) = t.ran.clone().expect("it ran");
    assert_eq!((prog.as_str(), args.len()), ("env", 0));
    let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    assert_eq!(get("VISIBLE_KEY"), Some(TRAP_VISIBLE));
    assert_eq!(get("HIDDEN_KEY"), None, "a secret of a Space is not handed over unasked");
    assert_eq!(get("WORK_KEY"), None, "nor another one of that Space");
    assert!(get("PATH").is_some(), "the rest of the environment comes along");
    for trap in [TRAP_VISIBLE, TRAP_HIDDEN, TRAP_OTHER] {
        assert!(!t.said().contains(trap), "{trap} was printed");
    }
}

#[test]
fn run_asks_before_handing_over_what_a_terminal_does_not_already_have() {
    let r = rig("run-asks");
    r.engine.lock().unwrap().vault_unlock("master").unwrap();
    // another Space
    let seen = person(&r.approvals, Decision::Deny);
    let mut t = Fake::default();
    assert_eq!(run(&r, &mut t, &["run", "-s", "work", "--", "printenv", "WORK_KEY"]), 1);
    assert!(t.ran.is_none() && t.said().contains("not allowed"), "{}", t.said());
    let shown = seen.lock().unwrap()[0].clone();
    assert!(shown.contains("printenv WORK_KEY") && shown.contains("work key (WORK_KEY)") && shown.contains("hidden (HIDDEN_KEY)") && !shown.contains(TRAP_OTHER) && !shown.contains(TRAP_HIDDEN), "the person is shown names, not values: {shown}");
    // allowed once
    let r2 = rig("run-asks-yes");
    r2.engine.lock().unwrap().vault_unlock("master").unwrap();
    person(&r2.approvals, Decision::Once);
    let mut t = Fake::default();
    assert_eq!(run(&r2, &mut t, &["run", "-s", "Work", "--", "sh", "-c", "echo hi"]), 0);
    let (_, args, env) = t.ran.unwrap();
    assert_eq!(args, vec!["-c", "echo hi"]);
    assert!(env.iter().any(|(k, v)| k == "WORK_KEY" && v == TRAP_OTHER) && env.iter().any(|(k, v)| k == "HIDDEN_KEY" && v == TRAP_HIDDEN));
    // locked vault, bad usage, unknown Space
    r2.engine.lock().unwrap().vault_lock();
    let mut t = Fake::default();
    assert_eq!(run(&r2, &mut t, &["run", "--", "env"]), 1);
    assert!(t.said().contains("oso unlock"));
    assert_eq!(run(&r2, &mut Fake::default(), &["run", "env"]), 2);
    assert_eq!(run(&r2, &mut Fake::default(), &["run", "--"]), 2);
    r2.engine.lock().unwrap().vault_unlock("master").unwrap();
    assert_eq!(run(&r2, &mut Fake::default(), &["run", "-s", "nope", "--", "env"]), 1);
}

#[test]
fn hosts_are_added_and_opened_from_the_command_line() {
    let r = rig("host");
    let mut t = Fake::default();
    assert_eq!(run(&r, &mut t, &["host", "add", "example", "example.example.com", "--user", "deploy", "--port", "2222", "--auth", "key", "--key", "~/.ssh/id_x", "--notes", "the API"]), 0, "{}", t.said());
    let s = r.engine.lock().unwrap().store.servers.iter().find(|s| s.name == "example").cloned().unwrap();
    assert_eq!((s.host.as_str(), s.user.as_str(), s.port, s.key_path.as_str(), s.notes.as_str()), ("example.example.com", "deploy", 2222, "~/.ssh/id_x", "the API"));
    assert_eq!(s.auth, oso_core::store::Auth::Key);
    assert_eq!(run(&r, &mut Fake::default(), &["host", "add", "EXAMPLE", "x"]), 1, "no two with the same name");
    assert_eq!(run(&r, &mut Fake::default(), &["host", "add", "z", "z.example", "--auth", "magic"]), 1);
    assert_eq!(run(&r, &mut Fake::default(), &["host", "add", "z", "z.example", "--bogus", "1"]), 2);
    let mut t = Fake::default();
    assert_eq!(run(&r, &mut t, &["ssh", "example"]), 0);
    let ev = r.events.lock().unwrap().iter().find(|(k, _)| k == "open").cloned().unwrap();
    assert_eq!(ev.1["server_id"], s.id);
    assert_eq!(run(&r, &mut Fake::default(), &["ssh", "nope"]), 1);
}

#[test]
fn no_command_has_a_way_to_print_a_secret() {
    // every command, with the vault open and secrets in it, and what each one printed
    let r = rig("noprint");
    r.engine.lock().unwrap().vault_unlock("master").unwrap();
    person(&r.approvals, Decision::Once);
    let mut all = String::new();
    for args in [vec!["status"], vec!["space"], vec!["space", "use", "work"], vec!["space", "use", "General"], vec!["host", "add", "x", "x.example"], vec!["ssh", "x"], vec!["run", "-s", "work", "--", "true"], vec!["lock"], vec!["bogus"], vec!["status", "--secrets"]] {
        let mut t = Fake { password: "master".into(), ..Default::default() };
        run(&r, &mut t, &args);
        all.push_str(&t.said());
    }
    for trap in [TRAP_VISIBLE, TRAP_HIDDEN, TRAP_OTHER] {
        assert!(!all.contains(trap), "{trap} printed: {all}");
    }
    assert!(cli::is_command(&["run".into()]) && !cli::is_command(&["daemon".into()]) && !cli::is_command(&[]));
}

// ---- oso scan

fn env_tree(r: &Rig) -> PathBuf {
    let d = r.dir.join("projects");
    std::fs::create_dir_all(d.join("app")).unwrap();
    std::fs::write(d.join("app/.env"), "PORT=80\nSTRIPE_KEY=sk_live_SCANTRAP1\nDB_PASSWORD=SCANTRAP-pass2\n").unwrap();
    std::fs::write(d.join("app/notes.txt"), "keep me").unwrap();
    d
}

#[test]
fn scan_lists_names_and_lengths_and_imports_only_when_asked() {
    let r = rig("scan");
    let d = env_tree(&r);
    let mut t = Fake::default();
    assert_eq!(run(&r, &mut t, &["scan", d.to_str().unwrap()]), 0);
    assert!(t.said().contains("STRIPE_KEY") && t.said().contains("17 characters") && t.said().contains("Nothing was imported"), "{}", t.said());
    assert!(!t.said().contains("SCANTRAP"), "no value is shown");
    assert!(r.engine.lock().unwrap().vault.list().iter().all(|m| !m.name.contains("app/")), "nothing came in");
    // with --import but "no"
    r.engine.lock().unwrap().vault_unlock("master").unwrap();
    let mut t = Fake { answers: vec![false], ..Default::default() };
    assert_eq!(run(&r, &mut t, &["scan", d.to_str().unwrap(), "--import"]), 1);
    assert!(!r.engine.lock().unwrap().vault.list().iter().any(|m| m.name == "app/STRIPE_KEY"));
    // yes to importing, no to deleting
    let mut t = Fake { answers: vec![true, false], ..Default::default() };
    assert_eq!(run(&r, &mut t, &["scan", d.to_str().unwrap(), "--import"]), 0, "{}", t.said());
    let names: Vec<String> = r.engine.lock().unwrap().vault.list().into_iter().map(|m| m.name).collect();
    assert!(names.contains(&"app/STRIPE_KEY".to_string()) && names.contains(&"app/DB_PASSWORD".to_string()), "{names:?}");
    assert!(!names.iter().any(|n| n.contains("PORT")), "a setting is not a secret");
    assert!(d.join("app/.env").exists(), "the original is kept unless you say");
    assert!(t.out.iter().any(|l| l.starts_with("? Delete the 1 original")), "{:?}", t.out);
    // importing again adds nothing
    let mut t = Fake { answers: vec![true, false], ..Default::default() };
    run(&r, &mut t, &["scan", d.to_str().unwrap(), "--import"]);
    assert!(t.said().contains("Imported 0 secret(s)"), "{}", t.said());
    // and yes to deleting removes the .env and only the .env
    let mut t = Fake { answers: vec![true, true], ..Default::default() };
    run(&r, &mut t, &["scan", d.to_str().unwrap(), "--import"]);
    assert!(!d.join("app/.env").exists() && d.join("app/notes.txt").exists());
    assert!(!t.said().contains("SCANTRAP"));
    assert_eq!(run(&r, &mut Fake::default(), &["scan"]), 2);
    let mut t = Fake::default();
    assert_eq!(run(&r, &mut t, &["scan", r.dir.join("nothing").to_str().unwrap()]), 0);
    assert!(t.said().contains("No .env file"));
}

#[test]
fn the_window_side_refuses_to_import_or_delete_outside_the_folder() {
    let r = rig("fence");
    r.engine.lock().unwrap().vault_unlock("master").unwrap();
    let d = env_tree(&r);
    let outside = r.dir.join("elsewhere");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join(".env"), "API_TOKEN=tok_OUTSIDE1234\n").unwrap();
    std::fs::write(d.join("app/passwords.txt"), "x").unwrap();
    let mut e = r.engine.lock().unwrap();
    let s = e.env_import(d.to_str().unwrap(), &[oso_core::envscan::EnvChoice { path: outside.join(".env").to_string_lossy().into(), names: vec![] }, oso_core::envscan::EnvChoice { path: d.join("app/passwords.txt").to_string_lossy().into(), names: vec![] }]).unwrap();
    assert!(s.added.is_empty() && s.skipped.len() == 2, "{s:?}");
    let gone = e.env_delete(d.to_str().unwrap(), &[outside.join(".env").to_string_lossy().into(), d.join("app/passwords.txt").to_string_lossy().into(), d.join("app").to_string_lossy().into()]).unwrap();
    assert!(gone.is_empty(), "{gone:?}");
    assert!(outside.join(".env").exists() && d.join("app/passwords.txt").exists());
}

#[test]
fn one_time_codes_come_from_a_seed_that_never_leaves_and_a_server_can_use_it() {
    let r = rig("totp");
    let mut e = r.engine.lock().unwrap();
    e.vault_unlock("master").unwrap();
    let meta = |name: &str, kind| SecretMeta { id: 0, name: name.into(), kind, env_var: None, agent_visible: false, notes: String::new(), updated: 0 };
    assert!(e.secret_save(meta("bad", SecretKind::Totp), Some("not a seed!")).is_err(), "a seed that cannot make codes is refused");
    let seed = "GEZDGNBVGY3TQOJQ";
    let id = e.secret_save(meta("github 2fa", SecretKind::Totp), Some(seed)).unwrap();
    let (code, left) = e.totp_code(id).unwrap();
    assert!(code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()) && (1..=30).contains(&left), "{code} {left}");
    assert!(!code.contains(seed) && !serde_json::to_string(&e.secret_list().unwrap()).unwrap().contains(seed));
    assert!(e.totp_code(9999).is_err());
    let plain = e.secret_save(meta("plain", SecretKind::Token), Some("a-token-value")).unwrap();
    assert!(e.totp_code(plain).is_err(), "only a seed makes codes");
    let server = e.save_server(oso_core::store::Server { name: "mfa".into(), host: "mfa.example".into(), port: 22, auth: oso_core::store::Auth::Agent, ..Default::default() }, None, None).unwrap();
    assert!(e.server_set_totp(server, Some(plain)).is_err(), "only a seed can be linked");
    e.server_set_totp(server, Some(id)).unwrap();
    assert!(e.store.server(server).unwrap().has_totp);
    e.secret_delete(id).unwrap();
    assert!(!e.store.server(server).unwrap().has_totp, "deleting the seed takes the flag down");
    e.vault_lock();
    assert!(e.totp_code(id).is_err());
}

#[test]
fn proton_pass_logins_come_in_with_their_two_factor_seeds_and_nothing_else() {
    let r = rig("proton");
    let file = r.dir.join("proton.csv");
    std::fs::write(&file, "type,name,url,email,username,password,note,totp,vault\nlogin,GitHub,https://github.com,me@x.com,me,PROTONTRAP-pw1,work,otpauth://totp/GitHub?secret=JBSWY3DPEHPK3PXP,Personal\nlogin,Mail,https://mail,m@x.com,,PROTONTRAP-pw2,,,Personal\nnote,Shopping,,,,,milk,,Personal\n").unwrap();
    let mut e = r.engine.lock().unwrap();
    let preview = e.proton_preview(file.to_str().unwrap()).unwrap();
    assert_eq!(preview.len(), 2);
    assert!(!serde_json::to_string(&preview).unwrap().contains("PROTONTRAP"), "a preview has no password");
    assert!(e.proton_import(file.to_str().unwrap(), &[0]).is_err(), "the vault is locked");
    e.vault_unlock("master").unwrap();
    let before = e.vault.list().len();
    let s = e.proton_import(file.to_str().unwrap(), &[0]).unwrap();
    assert_eq!(s.added, vec!["GitHub (me)", "GitHub (me) (2FA)"]);
    let list = e.vault.list();
    assert_eq!(list.len(), before + 2);
    assert!(list.iter().any(|m| m.name == "GitHub (me) (2FA)" && m.kind == SecretKind::Totp));
    assert!(list.iter().any(|m| m.name == "GitHub (me)" && m.notes.contains("https://github.com")));
    assert!(!list.iter().any(|m| m.name.starts_with("Mail")), "only what was chosen");
    assert!(std::fs::write(r.dir.join("bad.txt"), "nothing here").is_ok() && e.proton_preview(r.dir.join("bad.txt").to_str().unwrap()).is_err());
}
