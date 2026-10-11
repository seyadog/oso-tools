//! Importing keys and hosts from a `~/.ssh` folder (a throwaway one here, made with `ssh-keygen`).

mod common;

use common::which;
use oso_core::engine::{Emit, Engine, ImportKey, ImportRequest};
use oso_core::store::{Auth, Store};
use oso_core::vault::Vault;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("oso-import-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn keygen(file: &Path, kind: &str, pass: &str, extra: &[&str]) {
    let ok = Command::new("ssh-keygen").args(["-q", "-t", kind, "-N", pass, "-C", "someone@laptop"]).args(extra).arg("-f").arg(file).status().unwrap().success();
    assert!(ok);
}

/// A fake `~/.ssh` with three keys (one with a passphrase, one in the old PEM format) and a config.
fn ssh_dir(name: &str) -> Option<PathBuf> {
    which("ssh-keygen")?;
    let root = temp(name);
    let dir = root.join(".ssh");
    std::fs::create_dir_all(&dir).unwrap();
    keygen(&dir.join("id_work"), "ed25519", "", &[]);
    keygen(&dir.join("id_locked"), "ed25519", "s3cret-pass", &[]);
    keygen(&dir.join("id_old"), "rsa", "", &["-b", "2048", "-m", "PEM"]);
    std::fs::write(dir.join("known_hosts"), "x ssh-ed25519 AAAA\n").unwrap();
    std::fs::write(dir.join("not_a_key"), "hello").unwrap();
    let config = format!(
        "Host bastion\n  HostName bastion.example.com\n  User ops\n  IdentityFile {d}/id_work\n\nHost app\n  HostName app.internal\n  User deploy\n  Port 2200\n  IdentityFile {d}/id_locked\n  ProxyJump bastion\n\nHost legacy\n  HostName legacy.example.com\n  IdentityFile {d}/id_old\n\nHost plain\n  HostName plain.example.com\n  IdentityFile {d}/never-there\n\nHost *.corp\n  User x\n",
        d = dir.display()
    );
    std::fs::write(dir.join("config"), config).unwrap();
    Some(dir)
}

fn engine_in(dir: &Path) -> Engine {
    let emit: Emit = Arc::new(|_, _| {});
    let store = Store::load_from(dir.join("servers.json")).unwrap();
    let mut e = Engine::new(store, Vault::new(dir.join("vault.json")), emit);
    e.vault_unlock("master").unwrap();
    e
}

macro_rules! need_dir {
    ($n:expr) => {
        match ssh_dir($n) {
            Some(d) => d,
            None => return eprintln!("skipped: no ssh-keygen here"),
        }
    };
}

#[test]
fn the_preview_lists_keys_and_hosts_and_changes_nothing() {
    let dir = need_dir!("preview");
    let before: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| (e.file_name(), std::fs::metadata(e.path()).unwrap().len())).collect();
    let e = engine_in(dir.parent().unwrap());
    let p = e.import_preview(Some(dir.to_str().unwrap()));
    let names: Vec<&str> = p.keys.iter().map(|k| k.name.as_str()).collect();
    assert_eq!(names, vec!["id_locked", "id_old", "id_work"]);
    let locked = p.keys.iter().find(|k| k.name == "id_locked").unwrap();
    assert!(locked.encrypted && locked.fingerprint.starts_with("SHA256:") && locked.algorithm == "ssh-ed25519" && locked.unsupported.is_empty());
    assert!(!p.keys.iter().find(|k| k.name == "id_work").unwrap().encrypted);
    assert!(p.keys.iter().find(|k| k.name == "id_old").unwrap().unsupported.contains("PEM"));
    assert_eq!(p.hosts.iter().map(|h| h.alias.as_str()).collect::<Vec<_>>(), vec!["bastion", "app", "legacy", "plain"]);
    assert!(p.skipped.iter().any(|s| s.contains("*.corp")));
    let after: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| (e.file_name(), std::fs::metadata(e.path()).unwrap().len())).collect();
    assert_eq!(before, after, "the preview wrote nothing");
}

#[test]
fn importing_puts_keys_in_the_vault_and_hosts_in_the_list_and_keeps_every_original() {
    let dir = need_dir!("apply");
    let originals: Vec<String> = ["id_work", "id_locked", "id_old"].iter().map(|n| std::fs::read_to_string(dir.join(n)).unwrap()).collect();
    let mut e = engine_in(dir.parent().unwrap());
    let d = dir.to_str().unwrap();
    let req = ImportRequest {
        keys: vec![
            ImportKey { path: dir.join("id_work").to_string_lossy().into(), passphrase: None },
            ImportKey { path: dir.join("id_locked").to_string_lossy().into(), passphrase: Some("s3cret-pass".into()) },
            ImportKey { path: dir.join("id_old").to_string_lossy().into(), passphrase: None },
        ],
        hosts: vec!["app".into(), "legacy".into(), "plain".into(), "nope".into()],
    };
    let r = e.import_apply(Some(d), req).unwrap();
    assert_eq!(r.keys_added, vec!["id_work", "id_locked"]);
    assert!(r.problems.iter().any(|p| p.contains("id_old") && p.contains("kept as a file")), "{:?}", r.problems);
    assert!(r.problems.iter().any(|p| p.contains("nope")), "{:?}", r.problems);
    // hosts: app (and, because it needs it, bastion), legacy and plain
    let mut added = r.hosts_added.clone();
    added.sort();
    assert_eq!(added, vec!["app", "bastion", "legacy", "plain"]);
    let by = |n: &str| e.store.servers.iter().find(|s| s.name == n).unwrap().clone();
    let (app, bastion, plain) = (by("app"), by("bastion"), by("plain"));
    assert_eq!((app.host.as_str(), app.user.as_str(), app.port), ("app.internal", "deploy", 2200));
    assert_eq!(app.jump, Some(bastion.id), "the ProxyJump became a jump host");
    assert_eq!((app.auth, bastion.auth), (Auth::VaultKey, Auth::VaultKey), "hosts that use an imported key log in with it from the vault");
    assert!(e.vault.links(app.id).key.is_some() && e.vault.links(bastion.id).key.is_some());
    assert_ne!(e.vault.links(app.id).key, e.vault.links(bastion.id).key, "each its own key");
    assert_eq!(plain.auth, Auth::Agent, "a key file that is not there is not guessed at");
    // the key that came with a passphrase is in the vault without one, and works
    let ids = e.agent_identities();
    assert_eq!(ids.len(), 2);
    // the originals are exactly as they were
    for (n, text) in ["id_work", "id_locked", "id_old"].iter().zip(&originals) {
        assert_eq!(&std::fs::read_to_string(dir.join(n)).unwrap(), text, "{n} untouched");
    }
    assert!(dir.join("id_work.pub").exists());
    // importing again changes nothing
    let again = e.import_apply(Some(d), ImportRequest { keys: vec![ImportKey { path: dir.join("id_work").to_string_lossy().into(), passphrase: None }], hosts: vec!["app".into()] }).unwrap();
    assert!(again.keys_added.is_empty() && again.hosts_added.is_empty(), "{again:?}");
    assert_eq!(e.agent_identities().len(), 2);
}

#[test]
fn a_wrong_passphrase_is_a_sentence_not_a_vault_entry() {
    let dir = need_dir!("pass");
    let mut e = engine_in(dir.parent().unwrap());
    let r = e.import_apply(Some(dir.to_str().unwrap()), ImportRequest { keys: vec![ImportKey { path: dir.join("id_locked").to_string_lossy().into(), passphrase: Some("wrong".into()) }], hosts: vec![] }).unwrap();
    assert!(r.keys_added.is_empty() && r.problems[0].contains("wrong passphrase"), "{r:?}");
    assert!(e.secret_list().unwrap().is_empty());
}

#[test]
fn only_files_of_the_scanned_folder_can_be_imported_and_the_vault_must_be_open() {
    let dir = need_dir!("fence");
    let outside = temp("outside");
    keygen(&outside.join("id_other"), "ed25519", "", &[]);
    let mut e = engine_in(dir.parent().unwrap());
    let r = e.import_apply(Some(dir.to_str().unwrap()), ImportRequest { keys: vec![ImportKey { path: outside.join("id_other").to_string_lossy().into(), passphrase: None }], hosts: vec![] }).unwrap();
    assert!(r.keys_added.is_empty() && r.problems[0].contains("not inside"), "{r:?}");
    e.vault_lock();
    assert!(e.import_apply(Some(dir.to_str().unwrap()), ImportRequest { keys: vec![], hosts: vec![] }).is_err());
}

#[test]
fn deleting_originals_is_a_separate_step_that_only_touches_key_files_of_the_folder() {
    let dir = need_dir!("delete");
    let outside = temp("delete-outside");
    keygen(&outside.join("id_other"), "ed25519", "", &[]);
    let mut e = engine_in(dir.parent().unwrap());
    let d = dir.to_str().unwrap();
    let paths: Vec<String> = [dir.join("id_work"), dir.join("known_hosts"), dir.join("config"), dir.join("id_work.pub"), outside.join("id_other")].iter().map(|p| p.to_string_lossy().into()).collect();
    let gone = e.import_delete_originals(Some(d), &paths).unwrap();
    assert_eq!(gone, vec!["id_work"]);
    assert!(!dir.join("id_work").exists() && !dir.join("id_work.pub").exists());
    assert!(dir.join("known_hosts").exists() && dir.join("config").exists() && dir.join("id_locked").exists(), "nothing else is touched");
    assert!(outside.join("id_other").exists(), "nor anything outside the folder");
}
