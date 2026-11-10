use super::*;

/// Drive rows persisted by earlier releases carry the owner uid "test_user_uid";
/// they must keep resolving to the Personal space and back.
#[test]
fn local_user_owner_round_trips_through_personal_space() {
    let persisted_owner = Owner::User {
        user_uid: UserUid::new("test_user_uid"),
    };
    assert_eq!(Owner::local_user(), persisted_owner);
    assert_eq!(Space::from_owner(persisted_owner), Space::Personal);
    assert_eq!(Space::Personal.owner(), Some(persisted_owner));
}

#[test]
fn team_owner_maps_to_team_space() {
    let team_uid = ServerId::from(7);
    assert_eq!(
        Space::from_owner(Owner::Team { team_uid }),
        Space::Team { team_uid }
    );
    assert_eq!(
        Space::Team { team_uid }.owner(),
        Some(Owner::Team { team_uid })
    );
    assert_eq!(Space::Shared.owner(), None);
}
