use std::fmt;

use serde::{Deserialize, Serialize};
use uncaged_util::local_or_remote_path::LocalOrRemotePath;

/// An unique reference to a skill.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum SkillReference {
    /// A skill identified by the path to its SKILL.md file.
    Path(LocalOrRemotePath),
    /// A bundled skill distributed with Warp.
    BundledSkillId(String),
}

impl fmt::Display for SkillReference {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SkillReference::Path(path) => path.display_path().fmt(f),
            SkillReference::BundledSkillId(id) => write!(f, "@warp-skill:{id}"),
        }
    }
}

impl From<SkillReference> for uncaged_agent_api::skill_descriptor::SkillReference {
    fn from(reference: SkillReference) -> Self {
        match reference {
            SkillReference::Path(path) => {
                uncaged_agent_api::skill_descriptor::SkillReference::Path(path.display_path())
            }
            SkillReference::BundledSkillId(id) => {
                uncaged_agent_api::skill_descriptor::SkillReference::BundledSkillId(id)
            }
        }
    }
}
