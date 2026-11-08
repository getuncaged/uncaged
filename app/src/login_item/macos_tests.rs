use super::*;

#[test]
fn only_registered_statuses_are_unregistered() {
    // SMAppServiceStatus: NotRegistered, Enabled, RequiresApproval, NotFound.
    assert!(!is_registered(0));
    assert!(is_registered(1));
    assert!(is_registered(2));
    assert!(!is_registered(3));
}
