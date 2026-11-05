use std::path::PathBuf;

use futures::executor::block_on;
use string_offset::ByteOffset;

use super::*;

fn fragment(path: &str, content: &str) -> Fragment {
    Fragment::from_byte_range(
        content.to_string(),
        ContentHash::from_content(content),
        PathBuf::from(path),
        ByteOffset::from(0)..ByteOffset::from(content.len()),
    )
}

fn node(name: &str) -> NodeHash {
    NodeHash::from(&ContentHash::from_content(&format!("node:{name}")))
}

fn leaf(fragment: &Fragment) -> NodeHash {
    NodeHash::from(fragment.content_hash())
}

/// Syncs `fragments` under a single root node and returns that root.
async fn sync(store: &LocalStoreClient, root_name: &str, fragments: Vec<Fragment>) -> NodeHash {
    let root = node(root_name);
    let children = fragments.iter().map(leaf).collect();
    store
        .generate_embeddings(
            EmbeddingConfig::default(),
            fragments,
            root.clone(),
            RepoMetadata { path: None },
        )
        .await
        .unwrap();
    store
        .update_intermediate_nodes(
            EmbeddingConfig::default(),
            vec![IntermediateNode {
                hash: root.clone(),
                children,
            }],
        )
        .await
        .unwrap();
    root
}

async fn search(store: &LocalStoreClient, root: &NodeHash, query: &str) -> Vec<Fragment> {
    let hashes = store
        .get_relevant_fragments(
            EmbeddingConfig::default(),
            query.to_string(),
            root.clone(),
            RepoMetadata { path: None },
        )
        .await
        .unwrap();
    // Stand in for the index, which turns hashes back into fragments from disk.
    let fragments = hashes
        .iter()
        .map(|hash| {
            let state = store.state();
            let chunk = state.leaves.get(&NodeHash::from(hash)).unwrap();
            Fragment::from_byte_range(
                String::new(),
                chunk.content_hash.clone(),
                PathBuf::from(format!("/repo/{hash}")),
                ByteOffset::from(0)..ByteOffset::from(0),
            )
        })
        .collect();
    store
        .rerank_fragments(query.to_string(), fragments)
        .await
        .unwrap()
}

#[test]
fn reports_only_unknown_nodes_as_needing_sync() {
    block_on(async {
        let store = LocalStoreClient::new();
        let known = fragment("/repo/a.rs", "fn known() {}");
        let unknown = fragment("/repo/b.rs", "fn unknown() {}");
        let root = sync(&store, "root", vec![known.clone()]).await;

        let need = store
            .sync_merkle_tree(
                vec![root.clone(), leaf(&known), leaf(&unknown), node("other")],
                EmbeddingConfig::default(),
            )
            .await
            .unwrap();
        assert!(!need.contains(&root));
        assert!(!need.contains(&leaf(&known)));
        assert!(need.contains(&leaf(&unknown)));
        assert!(need.contains(&node("other")));
    });
}

#[test]
fn finds_the_relevant_chunk_under_the_root() {
    block_on(async {
        let store = LocalStoreClient::new();
        let drag = fragment(
            "/repo/app/src/workspace/tab_drag.rs",
            "fn begin_tab_drag(&mut self) {\n    self.dragging = true;\n}",
        );
        let theme = fragment(
            "/repo/app/src/appearance/theme.rs",
            "fn apply_theme(&mut self, theme: Theme) {}",
        );
        let root = sync(&store, "root", vec![drag.clone(), theme]).await;

        let results = search(&store, &root, "how are tabs dragged").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content_hash(), drag.content_hash());
    });
}

#[test]
fn search_is_scoped_to_the_given_root() {
    block_on(async {
        let store = LocalStoreClient::new();
        let ours = fragment("/repo-a/src/parser.rs", "fn parse_config() {}");
        let theirs = fragment("/repo-b/src/parser.rs", "fn parse_config_file() {}");
        let root_a = sync(&store, "a", vec![ours.clone()]).await;
        sync(&store, "b", vec![theirs]).await;

        let results = search(&store, &root_a, "parse config").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content_hash(), ours.content_hash());
    });
}

#[test]
fn unknown_words_return_nothing() {
    block_on(async {
        let store = LocalStoreClient::new();
        let root = sync(
            &store,
            "root",
            vec![fragment("/repo/a.rs", "fn render() {}")],
        )
        .await;
        assert!(search(&store, &root, "kubernetes operator")
            .await
            .is_empty());
    });
}

#[test]
fn rerank_caps_chunks_per_file() {
    block_on(async {
        let store = LocalStoreClient::new();
        let fragments: Vec<Fragment> = (0..6)
            .map(|i| fragment("/repo/big.rs", &format!("fn widget_{i}() {{ widget(); }}")))
            .collect();
        let root = sync(&store, "root", fragments.clone()).await;
        store
            .get_relevant_fragments(
                EmbeddingConfig::default(),
                "widget".to_string(),
                root,
                RepoMetadata { path: None },
            )
            .await
            .unwrap();

        let reranked = store
            .rerank_fragments("widget".to_string(), fragments)
            .await
            .unwrap();
        assert_eq!(reranked.len(), MAX_CHUNKS_PER_FILE);
    });
}

/// Indexes a real repository and prints the top hits for each query, to eyeball
/// ranking quality. Run with:
/// `UNCAGED_EVAL_REPO=/path/to/repo UNCAGED_EVAL_QUERIES="q1|q2" cargo nextest run -p ai eval_on_real_repo --run-ignored only --no-capture`
#[test]
#[ignore = "manual ranking check against a real repository"]
fn eval_on_real_repo() {
    use instant::Instant;

    use super::super::chunker::chunk_code;

    let Ok(repo) = std::env::var("UNCAGED_EVAL_REPO") else {
        return;
    };
    let queries = std::env::var("UNCAGED_EVAL_QUERIES").unwrap_or_default();

    let started = Instant::now();
    let mut vocab = Vocab::default();
    let mut chunks: Vec<(String, Document)> = Vec::new();
    let mut files = 0;
    for entry in ignore::WalkBuilder::new(&repo).build().flatten() {
        let path = entry.path();
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        if content.len() > 512 * 1024 {
            continue;
        }
        files += 1;
        for fragment in chunk_code(&content, path) {
            let label = format!(
                "{}:{}-{}",
                path.strip_prefix(&repo).unwrap_or(path).display(),
                fragment.start_line,
                fragment.end_line
            );
            chunks.push((
                label,
                Document::new(fragment.content, Some(path), &mut vocab),
            ));
        }
    }
    println!(
        "indexed {files} files, {} chunks, {} terms in {:?}",
        chunks.len(),
        vocab.len(),
        started.elapsed()
    );

    let docs: Vec<(&String, &Document)> = chunks.iter().map(|(label, doc)| (label, doc)).collect();
    for query in queries.split('|').filter(|query| !query.trim().is_empty()) {
        let started = Instant::now();
        let Some(parsed) = Query::new(query, &vocab) else {
            println!("\n{query}: no known words");
            continue;
        };
        let ranked = lexical::rank(&parsed, &docs, 5);
        println!("\n{query}  ({:?})", started.elapsed());
        for (label, score) in ranked {
            println!("  {score:6.2}  {label}");
        }
    }
}
