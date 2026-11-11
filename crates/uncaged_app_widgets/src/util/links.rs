use uncaged_core::channel::ChannelState;

pub const USER_DOCS_URL: &str = uncaged_app_theme::brand::README_URL;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const GITHUB_ISSUES_URL: &str = uncaged_app_theme::brand::ISSUES_URL;
pub const SLACK_URL: &str = uncaged_app_theme::brand::DISCUSSIONS_URL;
pub const PRIVACY_POLICY_URL: &str = uncaged_app_theme::brand::PRIVACY_URL;

pub fn feedback_form_url() -> String {
    let mut url =
        url::Url::parse(uncaged_app_theme::brand::NEW_ISSUE_URL).expect("Should not fail to parse");
    if let Some(version) = ChannelState::app_version() {
        url.query_pairs_mut().append_pair("warp-version", version);
    }
    url.query_pairs_mut()
        .append_pair("os-version", &os_info::get().version().to_string());
    url.to_string()
}
