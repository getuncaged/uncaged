use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ai::index::full_source_code_embedding::manager::{
    CodebaseIndexManager, CodebaseIndexManagerEvent,
};
use ai::index::full_source_code_embedding::RetrievalID;
use ai::index::lexical::{self, Document, Query, Vocab};
use ai::index::locations::CodeContextLocation;
use ai::index::FileSymbols;
use anyhow::anyhow;
use futures_util::stream::AbortHandle;
use warp_core::features::FeatureFlag;
use warpui::{AppContext, Entity, ModelContext, ModelHandle, SingletonEntity};

use crate::ai::agent::{AIAgentActionId, SearchCodebaseResult};
use crate::ai::outline::{OutlineStatus, RepoOutlines};
use crate::report_error;

#[derive(Debug)]
pub enum GetRelevantFilesControllerEvent {
    Success {
        action_id: AIAgentActionId,
        result: GetRelevantFilesControllerResult,
    },
    Error {
        action_id: AIAgentActionId,
    },
}

impl GetRelevantFilesControllerEvent {
    pub fn action_id(&self) -> &AIAgentActionId {
        match self {
            GetRelevantFilesControllerEvent::Success { action_id, .. } => action_id,
            GetRelevantFilesControllerEvent::Error { action_id } => action_id,
        }
    }
}

#[derive(Debug)]
pub enum GetRelevantFilesControllerResult {
    Locations(Arc<HashSet<CodeContextLocation>>),
    SearchResult(SearchCodebaseResult),
}

pub enum GetRelevantFilesRequestTarget {
    Local { directory: PathBuf },
}
#[derive(Debug, thiserror::Error)]
pub enum GetRelevantFilesError {
    #[error("Repo outline is still being computed.")]
    Pending,
    #[error("Failed to create outline.")]
    CreateFailed,
    #[error("Failed to create outline.")]
    Missing,
}

/// This enum allows us to use both the existing structure for outline-based indexing
/// and the new full source code indexing manager/model.
enum RequestHandle {
    /// Used with outline-based indexing.
    AbortHandle(AbortHandle),

    /// Used with full source code indexing.
    RetrievalID {
        repo_path: PathBuf,
        retrieval_id: RetrievalID,
    },
}

impl RequestHandle {
    fn abort(&mut self, ctx: &mut AppContext) {
        match self {
            RequestHandle::AbortHandle(abort_handle) => abort_handle.abort(),
            RequestHandle::RetrievalID {
                repo_path,
                retrieval_id,
            } => {
                CodebaseIndexManager::handle(ctx).update(ctx, |index_manager, ctx| {
                    if let Err(err) =
                        index_manager.abort_retrieval_request(repo_path, retrieval_id.clone(), ctx)
                    {
                        log::error!("Failed to abort file retrieval request: {err:?}");
                    }
                });
            }
        }
    }
}

/// Controller for GetRelevantFiles action. This is scoped per terminal session.
#[derive(Default)]
pub struct GetRelevantFilesController {
    /// Search requests currently in flight, keyed by the originating action ID.
    /// This allows several SearchCodebase actions to be active at once without newer requests
    /// cancelling unrelated older ones.
    pending_requests: std::collections::HashMap<AIAgentActionId, RequestHandle>,
}

impl GetRelevantFilesController {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let codebase_manager = CodebaseIndexManager::handle(ctx);
        ctx.subscribe_to_model(&codebase_manager, Self::handle_codebase_manager_event);
        Self::default()
    }

    fn pending_action_id_for_retrieval_id(
        &self,
        pending_retrieval_id: &RetrievalID,
    ) -> Option<&AIAgentActionId> {
        // Full-source embedding completion events only carry the retrieval ID, so map them back to
        // the agent action that initiated the request before emitting results.
        self.pending_requests
            .iter()
            .find_map(|(action_id, request_handle)| match request_handle {
                RequestHandle::AbortHandle(_) => None,
                RequestHandle::RetrievalID { retrieval_id, .. }
                    if retrieval_id == pending_retrieval_id =>
                {
                    Some(action_id)
                }
                RequestHandle::RetrievalID { .. } => None,
            })
    }

    fn handle_codebase_manager_event(
        &mut self,
        _: ModelHandle<CodebaseIndexManager>,
        codebase_manager_event: &CodebaseIndexManagerEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        match codebase_manager_event {
            CodebaseIndexManagerEvent::RetrievalRequestFailed {
                retrieval_id,
                error_message: error,
            } => {
                let Some(action_id) = self.pending_action_id_for_retrieval_id(retrieval_id) else {
                    return;
                };

                self.handle_relevant_file_paths_result(
                    Err(anyhow!(error.to_owned())),
                    action_id.clone(),
                    ctx,
                );
            }
            CodebaseIndexManagerEvent::RetrievalRequestCompleted {
                retrieval_id,
                fragments,
                ..
            } => {
                let Some(action_id) = self.pending_action_id_for_retrieval_id(retrieval_id) else {
                    return;
                };

                self.handle_relevant_file_paths_result(
                    Ok(fragments.clone()),
                    action_id.clone(),
                    ctx,
                );
            }
            _ => (),
        }
    }

    /// Start a new search query based on the repo outline.
    pub fn send_request(
        &mut self,
        target: GetRelevantFilesRequestTarget,
        query: String,
        partial_path_segments: Option<&Vec<String>>,
        action_id: AIAgentActionId,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), GetRelevantFilesError> {
        // Cancel any previous request for this action before dispatching.
        self.cancel_request_for_action(&action_id, ctx);
        match target {
            GetRelevantFilesRequestTarget::Local { directory } => {
                self.send_local_request(&directory, query, partial_path_segments, action_id, ctx)
            }
        }
    }

    fn send_local_request(
        &mut self,
        directory: &Path,
        query: String,
        partial_path_segments: Option<&Vec<String>>,
        action_id: AIAgentActionId,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), GetRelevantFilesError> {
        const MINIMUM_FILE_COUNT_FOR_RANKING: usize = 2;

        if FeatureFlag::FullSourceCodeEmbedding.is_enabled() {
            let codebase_mgr = CodebaseIndexManager::handle(ctx);
            if let Some(base_path) = codebase_mgr.as_ref(ctx).root_path_for_codebase(directory) {
                match codebase_mgr.update(ctx, |index_manager, ctx| {
                    index_manager.retrieve_relevant_files(query.clone(), base_path.as_path(), ctx)
                }) {
                    Ok(retrieval_request_id) => {
                        log::info!("Using full source code embedding for search");
                        self.pending_requests.insert(
                            action_id,
                            RequestHandle::RetrievalID {
                                repo_path: base_path.clone(),
                                retrieval_id: retrieval_request_id,
                            },
                        );

                        return Ok(());
                    }
                    Err(e) => {
                        log::info!(
                            "Failed to initiate full source code search: {e}, falling back to outline-based search"
                        );
                    }
                }
            }
        }

        match RepoOutlines::as_ref(ctx).get_outline(directory) {
            Some((OutlineStatus::Complete(outline), base_path)) => {
                // Uncaged: rank files on this machine by how well their path and
                // symbol names match the query. Warp sent the outline to its
                // servers for this.
                let file_outlines = outline.to_file_symbols(partial_path_segments);
                let locations = if file_outlines.len() < MINIMUM_FILE_COUNT_FOR_RANKING {
                    file_outlines
                        .into_iter()
                        .map(|file| CodeContextLocation::WholeFile(base_path.join(file.path)))
                        .collect()
                } else {
                    rank_outline_files(&query, file_outlines, &base_path)
                };
                ctx.emit(GetRelevantFilesControllerEvent::Success {
                    action_id,
                    result: GetRelevantFilesControllerResult::Locations(Arc::new(locations)),
                });
                Ok(())
            }
            Some((OutlineStatus::Pending, _)) => Err(GetRelevantFilesError::Pending),
            Some((OutlineStatus::Failed, _)) => Err(GetRelevantFilesError::CreateFailed),
            None => Err(GetRelevantFilesError::Missing),
        }
    }

    fn handle_relevant_file_paths_result(
        &mut self,
        relevant_file_locations: anyhow::Result<Arc<HashSet<CodeContextLocation>>>,
        action_id: AIAgentActionId,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.pending_requests.remove(&action_id).is_none() {
            return;
        }
        match relevant_file_locations {
            Ok(relevant_file_locations) => {
                ctx.emit(GetRelevantFilesControllerEvent::Success {
                    action_id,
                    result: GetRelevantFilesControllerResult::Locations(relevant_file_locations),
                });
            }
            Err(e) => {
                report_error!(anyhow!(e).context("get_relevant_files failed"));
                ctx.emit(GetRelevantFilesControllerEvent::Error { action_id });
            }
        };
    }

    /// Returns the path to the root directory for a codebase search where pwd is `directory`.
    pub fn root_directory_for_search(&self, directory: &Path, app: &AppContext) -> Option<PathBuf> {
        let mut start = None;
        if FeatureFlag::FullSourceCodeEmbedding.is_enabled() {
            start = CodebaseIndexManager::as_ref(app).root_path_for_codebase(directory);
        }
        start.or_else(|| {
            RepoOutlines::as_ref(app)
                .get_outline(directory)
                .map(|(_, root)| root)
        })
    }

    pub fn cancel_request_for_action(
        &mut self,
        action_id: &AIAgentActionId,
        ctx: &mut ModelContext<Self>,
    ) {
        if let Some(mut request_handle) = self.pending_requests.remove(action_id) {
            request_handle.abort(ctx);
        }
    }
}

impl Entity for GetRelevantFilesController {
    type Event = GetRelevantFilesControllerEvent;
}

/// Files the outline fallback returns at most.
const OUTLINE_RESULT_LIMIT: usize = 10;

/// Ranks outline files by keyword match against their path and symbol names.
fn rank_outline_files(
    query: &str,
    files: Vec<FileSymbols>,
    base_path: &Path,
) -> HashSet<CodeContextLocation> {
    let mut vocab = Vocab::default();
    let documents: Vec<(PathBuf, Document)> = files
        .into_iter()
        .map(|file| {
            let path = base_path.join(&file.path);
            let document = Document::new(&file.symbols, Some(Path::new(&file.path)), &mut vocab);
            (path, document)
        })
        .collect();
    let Some(query) = Query::new(query, &vocab) else {
        return HashSet::new();
    };
    let docs: Vec<(&PathBuf, &Document)> =
        documents.iter().map(|(path, doc)| (path, doc)).collect();
    lexical::rank(&query, &docs, OUTLINE_RESULT_LIMIT)
        .into_iter()
        .filter(|(path, _)| path.exists())
        .map(|(path, _)| CodeContextLocation::WholeFile(path.clone()))
        .collect()
}
