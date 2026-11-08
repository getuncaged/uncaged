use super::*;

#[test]
fn only_failure_leaves_the_removal_pending() {
    assert!(Outcome::NotRegistered.is_done());
    assert!(Outcome::Removed.is_done());
    assert!(!Outcome::Failed.is_done());
}
