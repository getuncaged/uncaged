use super::*;

const INSTALLED: [Origin; 3] = [Origin::System, Origin::Downloaded, Origin::Mine];

#[test]
fn outside_picking_cards_offer_use_and_mark_the_theme_in_use() {
    for origin in INSTALLED {
        assert_eq!(
            CardControl::for_card(origin, false, false, None),
            CardControl::Use("Use")
        );
        assert_eq!(
            CardControl::for_card(origin, false, true, None),
            CardControl::Current("In use")
        );
    }
    // The theme in use does nothing when clicked again.
    assert!(!CardControl::Current("In use").is_clickable(None));
    assert!(CardControl::Use("Use").is_clickable(None));
}

#[test]
fn picking_names_the_slot_on_every_installed_card() {
    for origin in INSTALLED {
        assert_eq!(
            CardControl::for_card(origin, false, false, Some(SystemTheme::Light)),
            CardControl::Use("Use for light")
        );
        assert_eq!(
            CardControl::for_card(origin, false, false, Some(SystemTheme::Dark)),
            CardControl::Use("Use for dark")
        );
        assert_eq!(
            CardControl::for_card(origin, false, true, Some(SystemTheme::Light)),
            CardControl::Current("Light theme")
        );
        assert_eq!(
            CardControl::for_card(origin, false, true, Some(SystemTheme::Dark)),
            CardControl::Current("Dark theme")
        );
    }
    // Choosing the slot's current theme again keeps it and goes back, so it stays clickable.
    assert!(CardControl::Current("Light theme").is_clickable(Some(SystemTheme::Light)));
}

#[test]
fn themes_not_yet_downloaded_are_got_first_in_either_mode() {
    for picking in [None, Some(SystemTheme::Light), Some(SystemTheme::Dark)] {
        assert_eq!(
            CardControl::for_card(Origin::Available, false, false, picking),
            CardControl::Get
        );
        assert_eq!(
            CardControl::for_card(Origin::Available, true, false, picking),
            CardControl::Installing
        );
        assert!(CardControl::Get.is_clickable(picking));
        assert!(!CardControl::Installing.is_clickable(picking));
    }
}

#[test]
fn the_banner_says_which_theme_is_being_chosen() {
    let light = picking_banner(SystemTheme::Light);
    assert!(light.starts_with("Choose your light theme"), "{light}");
    assert!(light.contains("light mode"), "{light}");

    let dark = picking_banner(SystemTheme::Dark);
    assert!(dark.starts_with("Choose your dark theme"), "{dark}");
    assert!(dark.contains("dark mode"), "{dark}");
}
