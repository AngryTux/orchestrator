//! Movement 10 — Namespaces: CRUD + scoping.

use orch_core::namespace::NamespaceManager;
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "orch-ns-{}-{}-{}",
        std::process::id(),
        name,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn create_namespace() {
    let dir = temp_dir("create");
    let mgr = NamespaceManager::new(dir.clone());
    mgr.create("production").unwrap();

    let list = mgr.list().unwrap();
    assert!(list.contains(&"production".to_string()));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn create_default_namespaces() {
    let dir = temp_dir("defaults");
    let mgr = NamespaceManager::new(dir.clone());
    mgr.init_defaults().unwrap();

    let list = mgr.list().unwrap();
    assert!(list.contains(&"default".to_string()));
    assert!(list.contains(&"secure".to_string()));
    assert!(list.contains(&"lab".to_string()));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn list_namespaces_sorted() {
    let dir = temp_dir("sorted");
    let mgr = NamespaceManager::new(dir.clone());
    mgr.create("zebra").unwrap();
    mgr.create("alpha").unwrap();
    mgr.create("middle").unwrap();

    let list = mgr.list().unwrap();
    assert_eq!(list, vec!["alpha", "middle", "zebra"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inspect_namespace() {
    let dir = temp_dir("inspect");
    let mgr = NamespaceManager::new(dir.clone());
    mgr.create("default").unwrap();

    let info = mgr.inspect("default").unwrap();
    assert!(info.is_some());
    assert_eq!(info.unwrap().name, "default");

    assert!(mgr.inspect("nonexistent").unwrap().is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_namespace() {
    let dir = temp_dir("delete");
    let mgr = NamespaceManager::new(dir.clone());
    mgr.create("temp").unwrap();
    assert!(mgr.list().unwrap().contains(&"temp".to_string()));

    mgr.delete("temp").unwrap();
    assert!(!mgr.list().unwrap().contains(&"temp".to_string()));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_nonexistent_fails() {
    let dir = temp_dir("del-noexist");
    let mgr = NamespaceManager::new(dir.clone());
    assert!(mgr.delete("ghost").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn duplicate_create_is_idempotent() {
    let dir = temp_dir("dup");
    let mgr = NamespaceManager::new(dir.clone());
    mgr.create("test").unwrap();
    mgr.create("test").unwrap(); // should not fail

    let list = mgr.list().unwrap();
    assert_eq!(list.iter().filter(|n| *n == "test").count(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rejects_invalid_namespace_name() {
    let dir = temp_dir("invalid");
    let mgr = NamespaceManager::new(dir.clone());
    assert!(mgr.create("../../etc").is_err());
    assert!(mgr.create("bad name").is_err());
    assert!(mgr.create("").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
