use std::collections::HashMap;
use std::ffi::OsString;

use super::gcp::GcpCloudProvider;
use super::{collect_env_vars, load_providers, CloudProvider};
use crate::ai::cloud_environments::{GcpProviderConfig, ProvidersConfig};

#[test]
fn extract_cloud_providers_empty_when_no_providers() {
    let config = ProvidersConfig {
        gcp: None,
        aws: None,
    };
    let providers = load_providers(&config, "run-1").unwrap();
    assert!(providers.is_empty());
}

#[test]
fn gcp_provider_env_vars() {
    let config = GcpProviderConfig {
        project_number: "123456789".to_string(),
        workload_identity_federation_pool_id: "my-pool".to_string(),
        workload_identity_federation_provider_id: "my-provider".to_string(),
        service_account_email: None,
    };
    let provider = GcpCloudProvider::new(&config, "run-99").unwrap();
    let vars = provider.env_vars().unwrap();

    assert!(vars.contains_key(&OsString::from("GOOGLE_APPLICATION_CREDENTIALS")));
    assert_eq!(
        vars.get(&OsString::from("GOOGLE_EXTERNAL_ACCOUNT_ALLOW_EXECUTABLES")),
        Some(&OsString::from("1"))
    );
}

#[test]
fn extract_cloud_providers_creates_gcp_provider() {
    let config = ProvidersConfig {
        gcp: Some(GcpProviderConfig {
            project_number: "111".to_string(),
            workload_identity_federation_pool_id: "pool".to_string(),
            workload_identity_federation_provider_id: "prov".to_string(),
            service_account_email: None,
        }),
        aws: None,
    };
    let providers = load_providers(&config, "run-1").unwrap();
    assert_eq!(providers.len(), 1);

    let vars = providers[0].env_vars().unwrap();
    assert!(vars.contains_key(&OsString::from("GOOGLE_APPLICATION_CREDENTIALS")));
}

#[test]
fn collect_provider_env_vars_merges_all_providers() {
    let config = ProvidersConfig {
        gcp: Some(GcpProviderConfig {
            project_number: "222".to_string(),
            workload_identity_federation_pool_id: "pool".to_string(),
            workload_identity_federation_provider_id: "prov".to_string(),
            service_account_email: None,
        }),
        aws: None,
    };
    let providers = load_providers(&config, "id-7").unwrap();
    assert_eq!(providers.len(), 1);

    let mut vars = HashMap::new();
    collect_env_vars(&providers, &mut vars).unwrap();

    // GCP variables.
    assert!(vars.contains_key(&OsString::from("GOOGLE_APPLICATION_CREDENTIALS")));
}
