use uncaged_ui::{Entity, ModelContext};
use uncaged_util::local_or_remote_path::LocalOrRemotePath;

use crate::workspace::view::global_search::view::GlobalSearchEvent;
use crate::workspace::view::global_search::SearchConfig;

pub struct GlobalSearch {}

impl Entity for GlobalSearch {
    type Event = GlobalSearchEvent;
}

impl GlobalSearch {
    pub fn new() -> Self {
        GlobalSearch {}
    }

    pub fn abort_search(&mut self) {}

    pub fn run_search(
        &mut self,
        _pattern: String,
        _roots: Vec<LocalOrRemotePath>,
        _search_config: SearchConfig,
        _ctx: &mut ModelContext<Self>,
    ) {
    }
}

impl Default for GlobalSearch {
    fn default() -> Self {
        Self::new()
    }
}
