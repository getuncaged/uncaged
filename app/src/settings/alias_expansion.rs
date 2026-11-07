use settings::macros::define_settings_group;
use settings::SupportedPlatforms;

define_settings_group!(AliasExpansionSettings, settings: [
    alias_expansion_enabled: AliasExpansionEnabled {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        private: false,
        toml_path: "terminal.input.alias_expansion_enabled",
        description: "Whether shell alias expansion is enabled in the input.",
    },
]);
