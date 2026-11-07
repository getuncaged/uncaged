use super::*;

#[test]
fn auto_indexing_requires_full_source_code_embedding_codebase_context_and_auto_indexing() {
    {
        let _flag = FeatureFlag::FullSourceCodeEmbedding.override_enabled(false);
        assert!(!codebase_auto_indexing_enabled(true, true,));
    }
    {
        let _flag = FeatureFlag::FullSourceCodeEmbedding.override_enabled(true);
        assert!(codebase_auto_indexing_enabled(true, true,));
        assert!(!codebase_auto_indexing_enabled(false, true,));
        assert!(!codebase_auto_indexing_enabled(true, false,));
        assert!(!codebase_auto_indexing_enabled(false, false,));
    }
}

#[test]
fn candidate_roots_are_deduped_before_filtering() {
    let roots = vec!["/repo", "/repo", "/other"];
    let candidates = auto_index_candidate_roots(roots, |root| *root != "/other");

    assert_eq!(candidates, vec!["/repo"]);
}
