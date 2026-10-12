//! Spaces through the engine: the groups of an older version become Spaces the first time, and are never imported twice.

use oso_core::engine::{Emit, Engine};
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
