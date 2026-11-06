//! Answers code searches from CLI agents (Claude Code, Gemini CLI, Codex) with the
//! on-device codebase index.
//!
//! The engine's MCP server ([`uncaged_engine::start_code_search_server`]) runs on its
//! own thread. Each search it receives comes here over a channel, runs through the
//! same [`CodebaseIndexManager`] the native `search_codebase` tool uses, and goes back
//! as text: the matching chunks with their paths and line numbers.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ::ai::index::full_source_code_embedding::manager::{
    CodebaseIndexManager, CodebaseIndexManagerEvent, RetrieveFileError,
};
use ::ai::index::full_source_code_embedding::RetrievalID;
use ::ai::index::locations::CodeContextLocation;
use futures::channel::{mpsc, oneshot};
use futures::FutureExt as _;
use uncaged_engine::{CodeSearchFn, CodeSearchRequest};
use warpui::{Entity, ModelContext, SingletonEntity};

/// Lines shown from one match before the rest is cut.
const MAX_LINES_PER_MATCH: usize = 80;
/// Lines shown from a file the index returned whole.
const MAX_LINES_WHOLE_FILE: usize = 40;
/// Roughly how much text one answer may hold, so it fits an agent's tool-output limit.
const MAX_ANSWER_CHARS: usize = 40_000;

type Reply = oneshot::Sender<Result<String, String>>;

/// A search waiting on the index.
struct PendingSearch {
    query: String,
    repo_root: PathBuf,
    reply: Reply,
}

pub struct UncagedCodeSearch {
    pending: HashMap<RetrievalID, PendingSearch>,
}

impl Entity for UncagedCodeSearch {
    type Event = ();
}

impl SingletonEntity for UncagedCodeSearch {}

impl UncagedCodeSearch {
    /// Start the MCP server and begin answering its searches. Call after
    /// [`CodebaseIndexManager`] is registered.
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let (requests_tx, requests_rx) = mpsc::unbounded::<(CodeSearchRequest, Reply)>();
        ctx.spawn_stream_local(
            requests_rx,
            |me, (request, reply), ctx| me.search(request, reply, ctx),
            |_, _| {},
        );
        ctx.subscribe_to_model(&CodebaseIndexManager::handle(ctx), |me, _, event, ctx| {
            me.on_index_event(event, ctx);
        });

        let search: CodeSearchFn = Arc::new(move |request| {
            let (reply_tx, reply_rx) = oneshot::channel();
            let sent = requests_tx.unbounded_send((request, reply_tx));
            async move {
                if sent.is_err() {
                    return Err("Uncaged's code search isn't running.".to_string());
                }
                reply_rx
                    .await
                    .unwrap_or_else(|_| Err("The search was cancelled.".to_string()))
            }
            .boxed()
        });
        uncaged_engine::start_code_search_server(search);

        Self {
            pending: HashMap::new(),
        }
    }

    fn search(&mut self, request: CodeSearchRequest, reply: Reply, ctx: &mut ModelContext<Self>) {
        let directory = PathBuf::from(&request.directory);
        let manager = CodebaseIndexManager::handle(ctx);
        let Some(repo_root) = manager.as_ref(ctx).root_path_for_codebase(&directory) else {
            let _ = reply.send(Err(format!(
                "{} isn't in Uncaged's code index. Uncaged indexes a repository when you open it in a \
                 terminal with codebase indexing on (Settings > Code). Search with your own tools for now.",
                directory.display()
            )));
            return;
        };

        let retrieval = manager.update(ctx, |manager, ctx| {
            manager.retrieve_relevant_files(request.query.clone(), &repo_root, ctx)
        });
        match retrieval {
            Ok(retrieval_id) => {
                self.pending.insert(
                    retrieval_id,
                    PendingSearch {
                        query: request.query,
                        repo_root,
                        reply,
                    },
                );
            }
            Err(err) => {
                let message = match err {
                    RetrieveFileError::IndexSyncing => format!(
                        "Uncaged is still indexing {}. Search with your own tools for now; \
                         this search works once indexing finishes.",
                        repo_root.display()
                    ),
                    RetrieveFileError::IndexNotFound => format!(
                        "{} isn't in Uncaged's code index. Search with your own tools for now.",
                        repo_root.display()
                    ),
                    RetrieveFileError::IndexFailed(err) => format!(
                        "Uncaged couldn't index {} ({err}). Search with your own tools instead.",
                        repo_root.display()
                    ),
                };
                let _ = reply.send(Err(message));
            }
        }
    }

    fn on_index_event(&mut self, event: &CodebaseIndexManagerEvent, ctx: &mut ModelContext<Self>) {
        match event {
            CodebaseIndexManagerEvent::RetrievalRequestCompleted {
                retrieval_id,
                fragments,
                ..
            } => {
                let Some(search) = self.pending.remove(retrieval_id) else {
                    return;
                };
                let locations: Vec<CodeContextLocation> = fragments.iter().cloned().collect();
                // Reading the files happens off the main thread.
                ctx.spawn(
                    async move {
                        let answer = render_matches(&search.query, &search.repo_root, locations);
                        (search.reply, answer)
                    },
                    |_, (reply, answer), _| {
                        let _ = reply.send(Ok(answer));
                    },
                );
            }
            CodebaseIndexManagerEvent::RetrievalRequestFailed {
                retrieval_id,
                error_message,
            } => {
                let Some(search) = self.pending.remove(retrieval_id) else {
                    return;
                };
                let _ = search.reply.send(Err(format!(
                    "The search failed ({error_message}). Search with your own tools instead."
                )));
            }
            CodebaseIndexManagerEvent::SyncStateUpdated { .. }
            | CodebaseIndexManagerEvent::IndexMetadataUpdated { .. }
            | CodebaseIndexManagerEvent::RemoveExpiredIndexMetadata { .. }
            | CodebaseIndexManagerEvent::NewIndexCreated { .. } => {}
        }
    }
}

/// The text an agent gets back: each match as a path, its line range and the lines
/// themselves, numbered from 1. Matches are grouped by file.
pub(crate) fn render_matches(
    query: &str,
    repo_root: &Path,
    mut locations: Vec<CodeContextLocation>,
) -> String {
    if locations.is_empty() {
        return format!(
            "No matches for \"{query}\" in {}. Try other keywords or identifiers.",
            repo_root.display()
        );
    }
    locations.sort_by(|a, b| a.path().cmp(b.path()));

    let mut out = format!(
        "Matches for \"{query}\" in {}, grouped by file:\n",
        repo_root.display()
    );
    let mut left_out = Vec::new();
    let mut seen_files = HashSet::new();
    for location in &locations {
        let path = location.path();
        let shown = path
            .strip_prefix(repo_root)
            .unwrap_or(path)
            .display()
            .to_string();
        if !seen_files.insert(path.clone()) {
            continue;
        }
        if out.len() >= MAX_ANSWER_CHARS {
            left_out.push(shown);
            continue;
        }
        let content = std::fs::read_to_string(path).ok();
        let lines: Vec<&str> = content
            .as_deref()
            .map(|c| c.lines().collect())
            .unwrap_or_default();
        match location {
            CodeContextLocation::Fragment(fragment) => {
                for range in &fragment.line_ranges {
                    render_range(&mut out, &shown, &lines, range.clone(), MAX_LINES_PER_MATCH);
                }
            }
            CodeContextLocation::WholeFile(_) => {
                render_range(
                    &mut out,
                    &shown,
                    &lines,
                    0..lines.len(),
                    MAX_LINES_WHOLE_FILE,
                );
            }
        }
    }
    if !left_out.is_empty() {
        out.push_str(&format!(
            "\nMore matching files, not shown to keep this short: {}\n",
            left_out.join(", ")
        ));
    }
    out
}

/// Append one match. `range` counts lines from 0 and excludes its end, the way the
/// index stores it.
fn render_range(
    out: &mut String,
    shown_path: &str,
    lines: &[&str],
    range: Range<usize>,
    max_lines: usize,
) {
    let start = range.start.min(lines.len());
    let end = range.end.min(lines.len());
    if start >= end {
        out.push_str(&format!(
            "\n{shown_path}\n(the file changed since it was indexed)\n"
        ));
        return;
    }
    out.push_str(&format!("\n{shown_path}:{}-{end}\n", start + 1));
    let shown_end = end.min(start + max_lines);
    for (index, line) in lines[start..shown_end].iter().enumerate() {
        out.push_str(&format!("{:>6}  {line}\n", start + index + 1));
    }
    if shown_end < end {
        out.push_str(&format!("   ... {} more lines\n", end - shown_end));
    }
}

#[cfg(test)]
#[path = "uncaged_code_search_tests.rs"]
mod tests;
