use clap::Parser;

use super::*;

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
