//! Spaces through the engine: the groups of an older version become Spaces the first time, and are never imported twice.

use oso_core::engine::{Emit, Engine};
use oso_core::vault::{SecretKind, SecretMeta};
use oso_core::store::Store;
use oso_core::vault::Vault;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("oso-spaces-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn engine_in(dir: &Path) -> Engine {
    let emit: Emit = Arc::new(|_, _| {});
    Engine::new(Store::load_from(dir.join("servers.json")).unwrap(), Vault::new(dir.join("vault.json")), emit)
}

#[test]
fn the_groups_of_an_older_version_become_spaces_when_the_engine_starts() {
    let dir = temp("groups");
    let fixture = include_str!("fixtures/groups.json");
    std::fs::write(dir.join("groups.json"), fixture).unwrap();
    let mut e = engine_in(&dir);
    let list = e.spaces_list();
    assert_eq!(list.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["ops", "just a shell"], "the group that does not match its arrangement is left out");
    let ops = &list[0];
    assert_eq!(ops.servers, vec![2, 3], "the servers of its terminals; this computer is not one");
    assert_eq!((ops.tabs.len(), ops.tabs[0].panes.len(), ops.tabs[0].broadcast), (1, 3, true));
    assert_eq!(ops.tabs[0].panes[2].command, "git status");
    assert_eq!(ops.tabs[0].shape["kind"], "split", "the arrangement is kept as it was");
    let report = e.spaces.take_migration().expect("the window is told once");
    assert_eq!((report.imported.len(), report.skipped), (2, vec!["edited by hand and wrong".to_string()]));
    // the old file is put aside, not deleted, and what is left is what a start without groups sees
    assert_eq!(std::fs::read_to_string(dir.join("groups.json.migrated")).unwrap(), fixture);
    assert!(!dir.join("groups.json").exists());
    // starting again does not import anything a second time, and keeps what was edited meanwhile
    let mut changed = list[1].clone();
    changed.name = "shell".into();
    e.space_save(&changed).unwrap();
    drop(e);
    std::fs::write(dir.join("groups.json"), fixture).unwrap();
    let mut again = engine_in(&dir);
    assert_eq!(again.spaces_list().iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["ops", "shell"]);
    assert!(again.spaces.take_migration().is_none());
    assert!(dir.join("groups.json").exists(), "a groups.json that comes back is left alone");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn without_groups_there_are_no_spaces_and_no_file() {
    let dir = temp("none");
    let e = engine_in(&dir);
    assert!(e.spaces_list().is_empty());
    assert!(!dir.join("oso-spaces.json").exists());
    std::fs::remove_dir_all(dir).ok();
}

fn secret(name: &str) -> SecretMeta {
    SecretMeta { id: 0, name: name.into(), kind: SecretKind::ApiKey, env_var: None, agent_visible: false, notes: String::new(), updated: 0 }
}

#[test]
fn the_secrets_of_the_old_profiles_move_to_their_spaces_when_the_vault_opens() {
    let dir = temp("profile-secrets");
    // a vault as the older version left it: secrets marked with their profile
    let (a, b, c, d) = {
        let mut e = engine_in(&dir);
        e.vault_unlock("m").unwrap();
        let ids = (e.secret_save(secret("work key"), Some("w")).unwrap(), e.secret_save(secret("home key"), Some("h")).unwrap(), e.secret_save(secret("default key"), Some("d")).unwrap(), e.secret_save(secret("for everyone"), Some("x")).unwrap());
        e.vault.set_legacy_mark(ids.0, "work").unwrap();
        e.vault.set_legacy_mark(ids.1, "home").unwrap();
        e.vault.set_legacy_mark(ids.2, "default").unwrap();
        ids
    };
    std::fs::write(dir.join("profiles.json"), r#"[{"id":"default","name":"default"},{"id":"work","name":"Work"},{"id":"home","name":"Home"}]"#).unwrap();
    std::fs::write(dir.join("servers.json"), r#"{"next_id":3,"folders":[],"servers":[]}"#).unwrap();
    let mut e = engine_in(&dir);
    let (work, home) = (e.spaces_list()[0].id, e.spaces_list()[1].id);
    assert!(e.spaces_list().iter().all(|s| s.secrets.is_empty()), "the vault is locked: nothing can be moved yet");
    assert!(std::fs::read_dir(&dir).unwrap().any(|f| f.unwrap().file_name().to_string_lossy().starts_with("backup-before-spaces")), "a copy of the vault was made first");
    e.vault_unlock("m").unwrap();
    assert_eq!(e.spaces.get(work).unwrap().secrets, vec![a]);
    assert_eq!(e.spaces.get(home).unwrap().secrets, vec![b]);
    assert_eq!(e.secret_space(c), None, "the default profile's are General's");
    assert_eq!(e.secret_space(d), None);
    assert_eq!(e.secrets_in_reach(None), vec![c, d], "General sees its own only");
    // the marks are gone from the vault, for good
    e.vault_lock();
    let mut v = oso_core::vault::Vault::new(dir.join("vault.json"));
    v.unlock("m").unwrap();
    assert!(v.legacy_marks().is_empty());
    // and it is not done again
    let mut again = engine_in(&dir);
    again.vault_unlock("m").unwrap();
    assert_eq!(again.spaces.get(work).unwrap().secrets, vec![a]);
    assert!(again.spaces.pending_secrets().is_empty());
    std::fs::remove_dir_all(dir).ok();
}
