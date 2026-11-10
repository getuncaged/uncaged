use super::*;

fn lazy_pattern(predicate: &AgentModeCommandExecutionPredicate) -> &LazyRegex {
    match &predicate.0 {
        AgentModeCommandExecutionPredicateType::AnchoredRegex(regex) => regex,
    }
}

#[test]
fn default_rules_compile_and_match_like_eager_regexes() {
    let commands = [
        "cat",
        "cat foo.txt",
        "catalog",
        "ls -la",
        "lsof",
        "find . -name x",
        "find",
        "which git",
        "rm -rf /",
        "rmdir x",
        "ssh host",
        "sh",
        "shellcheck",
        "zsh -c ls",
        "curl x",
        "",
    ];
    for rule in DEFAULT_COMMAND_EXECUTION_ALLOWLIST_RULES
        .iter()
        .chain(DEFAULT_COMMAND_EXECUTION_DENYLIST_RULES)
    {
        let predicate = AgentModeCommandExecutionPredicate::new_regex(rule).unwrap();
        assert!(
            lazy_pattern(&predicate).compiled.get().is_none(),
            "default rule {rule} should compile lazily"
        );
        let eager = Regex::new(&format!("^{rule}$")).unwrap();
        for command in commands {
            assert_eq!(
                predicate.matches(command),
                eager.is_match(command),
                "rule {rule} on {command:?}"
            );
        }
        assert!(lazy_pattern(&predicate).compiled.get().is_some());
    }
}

#[test]
fn custom_rules_are_validated_when_created() {
    let predicate = AgentModeCommandExecutionPredicate::new_regex("git status").unwrap();
    assert!(lazy_pattern(&predicate).compiled.get().is_some());
    assert!(predicate.matches("git status"));
    assert!(!predicate.matches("git status --short"));

    assert!(AgentModeCommandExecutionPredicate::new_regex("git (").is_err());
    assert!(
        serde_json::from_str::<AgentModeCommandExecutionPredicate>(
            r#"{"AnchoredRegex":"^git ($"}"#
        )
        .is_err()
    );
}

#[test]
fn predicates_serialize_compare_and_print_like_regexes() {
    let json = r#"{"AnchoredRegex":"^rm(\\s.*)?$"}"#;
    let predicate: AgentModeCommandExecutionPredicate = serde_json::from_str(json).unwrap();
    assert!(lazy_pattern(&predicate).compiled.get().is_none());
    assert_eq!(serde_json::to_string(&predicate).unwrap(), json);
    assert_eq!(
        predicate,
        AgentModeCommandExecutionPredicate::new_regex(r"rm(\s.*)?").unwrap()
    );
    assert_eq!(predicate.to_string(), r"rm(\s.*)?");
    assert_eq!(
        format!("{predicate:?}"),
        format!(
            "AgentModeCommandExecutionPredicate(AnchoredRegex({:?}))",
            Regex::new(r"^rm(\s.*)?$").unwrap()
        )
    );
    assert!(predicate.matches("rm -rf build"));
    assert!(!predicate.matches("rmdir build"));

    let custom: AgentModeCommandExecutionPredicate =
        serde_json::from_str(r#"{"AnchoredRegex":"^npm test$"}"#).unwrap();
    assert!(lazy_pattern(&custom).compiled.get().is_some());
    assert_ne!(custom, predicate);
}

/// The profile editor no longer shows computer-use rows, but profiles saved
/// with them must keep loading and keep their values on the next save.
#[test]
fn profiles_saved_with_computer_use_settings_still_load() {
    let json = r#"{
        "name": "Saved",
        "computer_use": "AlwaysAllow",
        "computer_use_model": "some-computer-use-model"
    }"#;
    let profile: AIExecutionProfile = serde_json::from_str(json).unwrap();
    assert_eq!(profile.name, "Saved");
    assert_eq!(profile.computer_use, ComputerUsePermission::AlwaysAllow);
    assert_eq!(
        profile.computer_use_model,
        Some(LLMId::from("some-computer-use-model"))
    );

    let round_tripped: AIExecutionProfile =
        serde_json::from_str(&serde_json::to_string(&profile).unwrap()).unwrap();
    assert_eq!(round_tripped, profile);
}
