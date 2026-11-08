use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::*;

/// An in-memory store whose contents the test can inspect after handing it over.
#[derive(Clone, Default)]
struct MemoryStore(Rc<RefCell<HashMap<String, String>>>);

impl MemoryStore {
    fn with(entries: &[(&str, &str)]) -> Self {
        let store = Self::default();
        for (key, value) in entries {
            store
                .0
                .borrow_mut()
                .insert(key.to_string(), value.to_string());
        }
        store
    }

    fn get(&self, key: &str) -> Option<String> {
        self.0.borrow().get(key).cloned()
    }
}

impl SecureStorage for MemoryStore {
    fn write_value(&self, key: &str, value: &str) -> Result<(), Error> {
        self.0.borrow_mut().insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn read_value(&self, key: &str) -> Result<String, Error> {
        self.get(key).ok_or(Error::NotFound)
    }

    fn remove_value(&self, key: &str) -> Result<(), Error> {
        self.0
            .borrow_mut()
            .remove(key)
            .map(|_| ())
            .ok_or(Error::NotFound)
    }
}

fn storage(current: &MemoryStore, legacy: &MemoryStore) -> WithLegacyService {
    WithLegacyService {
        current: Box::new(current.clone()),
        legacy: Box::new(legacy.clone()),
    }
}

#[test]
fn reads_and_copies_forward_a_value_only_the_previous_service_has() {
    let current = MemoryStore::default();
    let legacy = MemoryStore::with(&[("FileBasedMcpCredentials", "token")]);

    let value = storage(&current, &legacy).read_value("FileBasedMcpCredentials");

    assert_eq!(value.unwrap(), "token");
    assert_eq!(
        current.get("FileBasedMcpCredentials").as_deref(),
        Some("token")
    );
    assert_eq!(
        legacy.get("FileBasedMcpCredentials").as_deref(),
        Some("token"),
        "the previous service keeps its copy for a downgrade"
    );
}

#[test]
fn the_current_value_wins() {
    let current = MemoryStore::with(&[("key", "new")]);
    let legacy = MemoryStore::with(&[("key", "old")]);

    assert_eq!(storage(&current, &legacy).read_value("key").unwrap(), "new");
}

#[test]
fn missing_from_both_is_not_found() {
    let current = MemoryStore::default();
    let legacy = MemoryStore::default();

    assert!(matches!(
        storage(&current, &legacy).read_value("key"),
        Err(Error::NotFound)
    ));
}

#[test]
fn writes_go_to_the_current_service_only() {
    let current = MemoryStore::default();
    let legacy = MemoryStore::with(&[("key", "old")]);

    storage(&current, &legacy)
        .write_value("key", "new")
        .unwrap();

    assert_eq!(current.get("key").as_deref(), Some("new"));
    assert_eq!(legacy.get("key").as_deref(), Some("old"));
}

#[test]
fn a_removed_value_does_not_come_back_from_the_previous_service() {
    let current = MemoryStore::default();
    let legacy = MemoryStore::with(&[("key", "old")]);
    let storage = storage(&current, &legacy);
    storage.read_value("key").unwrap();

    storage.remove_value("key").unwrap();

    assert!(matches!(storage.read_value("key"), Err(Error::NotFound)));
    assert!(legacy.get("key").is_none());
}

#[test]
fn removing_a_value_only_the_previous_service_has_succeeds() {
    let current = MemoryStore::default();
    let legacy = MemoryStore::with(&[("key", "old")]);

    storage(&current, &legacy).remove_value("key").unwrap();

    assert!(legacy.get("key").is_none());
}
