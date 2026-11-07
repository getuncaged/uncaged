use std::ffi::OsString;

use clap::Parser;

use super::*;

fn set_env_var(name: &str, value: &str) -> Option<OsString> {
    let previous = std::env::var_os(name);
    // Safety: tests that mutate process environment are marked `serial` so we
    // do not race with other environment readers/writers in this crate.
    unsafe { std::env::set_var(name, value) };
    previous
}

fn restore_env_var(name: &str, previous: Option<OsString>) {
    match previous {
        // Safety: tests that mutate process environment are marked `serial` so
        // we do not race with other environment readers/writers in this crate.
        Some(value) => unsafe { std::env::set_var(name, value) },
        // Safety: tests that mutate process environment are marked `serial` so
        // we do not race with other environment readers/writers in this crate.
        None => unsafe { std::env::remove_var(name) },
    }
}

#[test]
#[serial_test::serial]
fn hidden_server_overrides_parse_from_env() {
    let previous_server_root = set_env_var(SERVER_ROOT_URL_OVERRIDE_ENV, "http://localhost:8080");
    let previous_ws = set_env_var(WS_SERVER_URL_OVERRIDE_ENV, "ws://localhost:8082/graphql/v2");

    let args = Args::try_parse_from(["warp"]).unwrap();

    restore_env_var(SERVER_ROOT_URL_OVERRIDE_ENV, previous_server_root);
    restore_env_var(WS_SERVER_URL_OVERRIDE_ENV, previous_ws);

    assert_eq!(args.server_root_url(), Some("http://localhost:8080"));
    assert_eq!(args.ws_server_url(), Some("ws://localhost:8082/graphql/v2"));
}

#[test]
fn worker_and_app_flags_resolve() {
    assert_eq!(finish_update_flag(), "--finish-update");
    assert_eq!(dump_debug_info_flag(), "--dump-debug-info");
    #[cfg(unix)]
    assert_eq!(terminal_server_subcommand(), "terminal-server");
}

#[test]
fn dump_debug_info_parses() {
    let args = Args::try_parse_from(["warp", "--dump-debug-info"]).unwrap();
    assert!(matches!(args.command(), Some(Command::DumpDebugInfo)));
}

#[test]
fn removed_cloud_agent_subcommands_are_rejected() {
    for subcommand in ["agent", "environment", "schedule", "secret", "run", "login"] {
        assert!(
            Args::try_parse_from(["warp", subcommand]).is_err(),
            "`{subcommand}` should no longer parse"
        );
    }
}
