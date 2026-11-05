//! Uncaged's on-device replacement for Warp's remote embedding store.
//!
//! The codebase index walks a repository, splits files into chunks and syncs a
//! Merkle tree of them to a [`StoreClient`]. Warp's store embedded each chunk on
//! its servers; this one keeps everything in memory on this machine and ranks
//! chunks by keyword relevance (see [`crate::index::lexical`]). Nothing leaves
//! the machine and nothing costs money.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;

use super::store_client::{IntermediateNode, StoreClient};
use super::{
    CodebaseContextConfig, ContentHash, EmbeddingConfig, Error, Fragment, NodeHash, RepoMetadata,
};
use crate::index::lexical::{self, Document, Query, Vocab};

/// How many chunks a search hands to the rerank step.
const CANDIDATE_LIMIT: usize = 40;
/// How many chunks a search returns in the end.
const RESULT_LIMIT: usize = 12;
/// At most this many chunks from one file, so one big file can't crowd out the rest.
const MAX_CHUNKS_PER_FILE: usize = 3;
/// How often the index picks up file changes. Keyword indexing is cheap, so
/// this is much shorter than the remote store's cadence.
const SYNC_CADENCE: Duration = Duration::from_secs(15);
/// Recent searches whose scores are kept for the rerank step.
const REMEMBERED_SEARCHES: usize = 8;
/// Roots whose chunk lists are kept between searches.
const REMEMBERED_ROOTS: usize = 8;

#[derive(Default)]
pub struct LocalStoreClient {
    state: Mutex<LocalStore>,
}

impl LocalStoreClient {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, LocalStore> {
        // A panic while holding the lock leaves plain data behind; keep going.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Default)]
struct LocalStore {
    vocab: Vocab,
    /// Children of every intermediate node the index has synced.
    intermediate: HashMap<NodeHash, Vec<NodeHash>>,
    /// Every chunk the index has synced, keyed by its leaf hash.
    leaves: HashMap<NodeHash, IndexedChunk>,
    /// Leaf hashes reachable from recently searched roots.
    roots: VecDeque<(NodeHash, Arc<Vec<NodeHash>>)>,
    /// Scores from recent searches, used to order the rerank step.
    searches: VecDeque<(String, HashMap<ContentHash, f32>)>,
}

struct IndexedChunk {
    content_hash: ContentHash,
    document: Document,
}

impl LocalStore {
    fn knows(&self, node: &NodeHash) -> bool {
        self.intermediate.contains_key(node) || self.leaves.contains_key(node)
    }

    /// Leaf hashes under `root`, cached for repeated searches on the same tree.
    fn leaves_under(&mut self, root: &NodeHash) -> Arc<Vec<NodeHash>> {
        if let Some((_, leaves)) = self.roots.iter().find(|(hash, _)| hash == root) {
            return leaves.clone();
        }

        let mut found = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![root.clone()];
        while let Some(node) = stack.pop() {
            if !seen.insert(node.clone()) {
                continue;
            }
            match self.intermediate.get(&node) {
                Some(children) => stack.extend(children.iter().cloned()),
                None => {
                    if self.leaves.contains_key(&node) {
                        found.push(node);
                    }
                }
            }
        }

        let found = Arc::new(found);
        self.roots.push_front((root.clone(), found.clone()));
        self.roots.truncate(REMEMBERED_ROOTS);
        found
    }

    fn search(&mut self, query: &str, root: &NodeHash) -> Vec<(ContentHash, f32)> {
        let Some(parsed) = Query::new(query, &self.vocab) else {
            return Vec::new();
        };
        let leaves = self.leaves_under(root);
        let docs: Vec<(&ContentHash, &Document)> = leaves
            .iter()
            .filter_map(|leaf| self.leaves.get(leaf))
            .map(|chunk| (&chunk.content_hash, &chunk.document))
            .collect();

        // Identical chunks in different files share a content hash; rank each once.
        let mut seen = HashSet::new();
        let ranked: Vec<(ContentHash, f32)> = lexical::rank(&parsed, &docs, CANDIDATE_LIMIT * 2)
            .into_iter()
            .filter(|(hash, _)| seen.insert((*hash).clone()))
            .take(CANDIDATE_LIMIT)
            .map(|(hash, score)| (hash.clone(), score))
            .collect();

        self.searches.retain(|(remembered, _)| remembered != query);
        self.searches
            .push_front((query.to_string(), ranked.iter().cloned().collect()));
        self.searches.truncate(REMEMBERED_SEARCHES);
        ranked
    }

    fn score_for(&self, query: &str, hash: &ContentHash) -> f32 {
        self.searches
            .iter()
            .find(|(remembered, _)| remembered == query)
            .and_then(|(_, scores)| scores.get(hash).copied())
            .unwrap_or(0.0)
    }

    fn forget_cached_roots(&mut self) {
        self.roots.clear();
    }
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl StoreClient for LocalStoreClient {
    async fn update_intermediate_nodes(
        &self,
        _embedding_config: EmbeddingConfig,
        nodes: Vec<IntermediateNode>,
    ) -> Result<HashMap<NodeHash, bool>, Error> {
        let mut state = self.state();
        let mut results = HashMap::with_capacity(nodes.len());
        for node in nodes {
            results.insert(node.hash.clone(), true);
            state.intermediate.insert(node.hash, node.children);
        }
        state.forget_cached_roots();
        Ok(results)
    }

    async fn generate_embeddings(
        &self,
        _embedding_config: EmbeddingConfig,
        fragments: Vec<Fragment>,
        _root_hash: NodeHash,
        _repo_metadata: RepoMetadata,
    ) -> Result<HashMap<ContentHash, bool>, Error> {
        let mut state = self.state();
        let mut results = HashMap::with_capacity(fragments.len());
        for fragment in fragments {
            let document = Document::new(
                &fragment.content,
                Some(fragment.absolute_path()),
                &mut state.vocab,
            );
            let content_hash = fragment.content_hash().clone();
            state.leaves.insert(
                NodeHash::from(&content_hash),
                IndexedChunk {
                    content_hash: content_hash.clone(),
                    document,
                },
            );
            results.insert(content_hash, true);
        }
        state.forget_cached_roots();
        Ok(results)
    }

    async fn populate_merkle_tree_cache(
        &self,
        _embedding_config: EmbeddingConfig,
        _root_hash: NodeHash,
        _repo_metadata: RepoMetadata,
    ) -> Result<bool, Error> {
        // Nothing to warm: the tree already lives in memory.
        Ok(true)
    }

    async fn sync_merkle_tree(
        &self,
        nodes: Vec<NodeHash>,
        _embedding_config: EmbeddingConfig,
    ) -> Result<HashSet<NodeHash>, Error> {
        // Report the nodes this store hasn't seen yet; the index sends those.
        let state = self.state();
        Ok(nodes
            .into_iter()
            .filter(|node| !state.knows(node))
            .collect())
    }

    async fn rerank_fragments(
        &self,
        query: String,
        fragments: Vec<Fragment>,
    ) -> Result<Vec<Fragment>, Error> {
        let state = self.state();
        let mut scored: Vec<(f32, Fragment)> = fragments
            .into_iter()
            .map(|fragment| (state.score_for(&query, fragment.content_hash()), fragment))
            .collect();
        drop(state);
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));

        let mut per_file: HashMap<std::path::PathBuf, usize> = HashMap::new();
        let mut results = Vec::with_capacity(RESULT_LIMIT);
        for (_, fragment) in scored {
            let count = per_file
                .entry(fragment.absolute_path().to_path_buf())
                .or_default();
            if *count >= MAX_CHUNKS_PER_FILE {
                continue;
            }
            *count += 1;
            results.push(fragment);
            if results.len() == RESULT_LIMIT {
                break;
            }
        }
        Ok(results)
    }

    async fn get_relevant_fragments(
        &self,
        _embedding_config: EmbeddingConfig,
        query: String,
        root_hash: NodeHash,
        _repo_metadata: RepoMetadata,
    ) -> Result<Vec<ContentHash>, Error> {
        let mut state = self.state();
        Ok(state
            .search(&query, &root_hash)
            .into_iter()
            .map(|(hash, _)| hash)
            .collect())
    }

    async fn codebase_context_config(&self) -> Result<CodebaseContextConfig, Error> {
        Ok(CodebaseContextConfig {
            embedding_config: EmbeddingConfig::default(),
            embedding_cadence: SYNC_CADENCE,
        })
    }
}

#[cfg(test)]
#[path = "local_store_tests.rs"]
mod tests;
