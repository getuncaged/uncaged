use dirs::home_dir;

use super::*;

#[test]
fn test_data_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    // ChannelState, by default, is configured for Channel::Oss.
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(data_dir(), home_dir.join(".uncaged"));
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(data_dir(), home_dir.join(".local/share/uncaged"));
        } else if #[cfg(windows)] {
            assert_eq!(data_dir(), home_dir.join("AppData\\Roaming\\uncaged\\Uncaged\\data"));
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_config_local_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    // ChannelState, by default, is configured for Channel::Oss.
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(config_local_dir(), home_dir.join(".uncaged"));
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(config_local_dir(), home_dir.join(".config/uncaged"));
        } else if #[cfg(windows)] {
            assert_eq!(config_local_dir(), home_dir.join("AppData\\Local\\uncaged\\Uncaged\\config"));
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_warp_home_config_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    let expected_dir_name = match ChannelState::data_profile() {
        Some(data_profile) => format!(".uncaged-{data_profile}"),
        None => ".uncaged".to_string(),
    };

    assert_eq!(
        warp_home_config_dir(),
        Some(home_dir.join(expected_dir_name))
    );
}

#[test]
fn test_warp_home_skills_and_mcp_paths() {
    let Some(config_dir) = warp_home_config_dir() else {
        panic!("Should be able to compute Warp home config directory");
    };

    assert_eq!(warp_home_skills_dir(), Some(config_dir.join("skills")));
    assert_eq!(
        warp_home_mcp_config_file_path(),
        Some(config_dir.join(".mcp.json"))
    );
}
#[test]
fn test_cache_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    // ChannelState, by default, is configured for Channel::Oss.
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(cache_dir(), home_dir.join("Library/Application Support/dev.uncaged.Uncaged"));
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(cache_dir(), home_dir.join(".cache/uncaged"));
        } else if #[cfg(windows)] {
            assert_eq!(cache_dir(), home_dir.join("AppData\\Local\\uncaged\\Uncaged\\cache"));
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_state_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    cfg_if::cfg_if! {
        // ChannelState, by default, is configured for Channel::Oss.
        if #[cfg(target_os = "macos")] {
            assert_eq!(state_dir(), home_dir.join("Library/Application Support/dev.uncaged.Uncaged"));
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(state_dir(), home_dir.join(".local/state/uncaged"));
        } else if #[cfg(windows)] {
            assert_eq!(state_dir(), home_dir.join("AppData\\Local\\uncaged\\Uncaged\\data"));
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_project_path_for_warp_app_id() {
    let project_dirs = project_dirs_for_app_id(AppId::new("dev", "warp", "Warp"), None)
        .expect("should be able to compute project dirs");
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(project_dirs.project_path(), "dev.warp.Warp");
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(project_dirs.project_path(), "warp-terminal");
        } else if #[cfg(windows)] {
            assert_eq!(project_dirs.project_path(), "warp\\Warp");
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_project_path_for_warp_dev_app_id() {
    let project_dirs = project_dirs_for_app_id(AppId::new("dev", "warp", "WarpDev"), None)
        .expect("should be able to compute project dirs");
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(project_dirs.project_path(), "dev.warp.WarpDev");
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(project_dirs.project_path(), "warp-terminal-dev");
        } else if #[cfg(windows)] {
            assert_eq!(project_dirs.project_path(), "warp\\WarpDev");
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_project_path_for_renamed_oss_app_id() {
    let project_dirs = project_dirs_for_app_id(AppId::new("dev", "uncaged", "Uncaged"), None)
        .expect("should be able to compute project dirs");
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(project_dirs.project_path(), "dev.uncaged.Uncaged");
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(project_dirs.project_path(), "uncaged");
        } else if #[cfg(windows)] {
            assert_eq!(project_dirs.project_path(), "uncaged\\Uncaged");
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

/// The identity Uncaged used up to v0.3.4, whose data the first launch moves.
#[test]
fn test_project_path_for_oss_app_id() {
    let project_dirs = project_dirs_for_app_id(AppId::new("dev", "uncaged", "WarpOss"), None)
        .expect("should be able to compute project dirs");
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(project_dirs.project_path(), "dev.uncaged.WarpOss");
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(project_dirs.project_path(), "warp-oss");
        } else if #[cfg(windows)] {
            assert_eq!(project_dirs.project_path(), "uncaged\\WarpOss");
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_identity_dirs_for_oss_app_ids() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    let legacy = identity_dirs(AppId::new("dev", "uncaged", "WarpOss"), None)
        .expect("should be able to compute identity dirs");
    let renamed = identity_dirs(AppId::new("dev", "uncaged", "Uncaged"), Some("renametest"))
        .expect("should be able to compute identity dirs");
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            let library = home_dir.join("Library");
            assert_eq!(legacy.state_dir, library.join("Application Support/dev.uncaged.WarpOss"));
            assert_eq!(
                legacy.roots,
                vec![
                    library.join("Application Support/dev.uncaged.WarpOss"),
                    library.join("Caches/dev.uncaged.WarpOss"),
                    library.join("Saved Application State/dev.uncaged.WarpOss.savedState"),
                ]
            );
            assert_eq!(
                renamed.roots,
                vec![
                    library.join("Application Support/dev.uncaged.Uncaged-renametest"),
                    library.join("Caches/dev.uncaged.Uncaged-renametest"),
                    library.join("Saved Application State/dev.uncaged.Uncaged-renametest.savedState"),
                ]
            );
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(legacy.state_dir, home_dir.join(".local/state/warp-oss"));
            assert_eq!(
                legacy.roots,
                vec![
                    home_dir.join(".local/state/warp-oss"),
                    home_dir.join(".local/share/uncaged"),
                    home_dir.join(".config/uncaged"),
                    home_dir.join(".cache/warp-oss"),
                ]
            );
            assert_eq!(renamed.state_dir, home_dir.join(".local/state/uncaged-renametest"));
            assert_eq!(renamed.roots[3], home_dir.join(".cache/uncaged-renametest"));
        } else if #[cfg(windows)] {
            assert_eq!(legacy.state_dir, home_dir.join("AppData\\Local\\uncaged\\WarpOss\\data"));
            assert_eq!(
                legacy.roots,
                vec![
                    home_dir.join("AppData\\Local\\uncaged\\WarpOss"),
                    home_dir.join("AppData\\Roaming\\uncaged\\WarpOss"),
                ]
            );
            assert_eq!(renamed.roots[0], home_dir.join("AppData\\Local\\uncaged\\Uncaged-renametest"));
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
    // The database lives under the first root, which is moved first.
    assert!(legacy.state_dir.starts_with(&legacy.roots[0]));
}
