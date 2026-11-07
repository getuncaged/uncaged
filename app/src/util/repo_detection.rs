//! Helpers for constructing repo detection calls from view contexts.
//!
//! The core detection logic lives on
//! [`DetectedRepositories::detect_possible_git_repo`]. This module provides
//! a thin convenience layer over it.

use std::future::Future;

use futures::future::ready;
#[cfg(not(target_family = "wasm"))]
use futures::future::{Either, Ready};
#[cfg(not(target_family = "wasm"))]
use repo_metadata::repositories::DetectedRepositories;
use repo_metadata::repositories::RepoDetectionSource;
use warp_util::local_or_remote_path::LocalOrRemotePath;
#[cfg(not(target_family = "wasm"))]
use warp_util::remote_path::RemoteNavigationResult;
#[cfg(not(target_family = "wasm"))]
use warpui::SingletonEntity;
use warpui::{View, ViewContext};

/// Describes whether the active session is local or remote.
pub enum RepoDetectionSessionType {
    /// A local terminal session — repo detection runs on the local filesystem.
    Local,
    /// A remote SSH session — repo detection is unavailable.
    Remote,
}

/// Detects the git repository root for the given working directory.
///
/// Remote sessions resolve to `None` immediately; this avoids falling through
/// to local detection, which would misclassify a remote CWD as a local repo if
/// the same absolute path happens to exist locally.
#[cfg(not(target_family = "wasm"))]
pub fn detect_possible_git_repo<V: View>(
    session_type: RepoDetectionSessionType,
    active_directory: &str,
    source: RepoDetectionSource,
    ctx: &mut ViewContext<V>,
) -> impl Future<Output = Option<LocalOrRemotePath>> {
    match session_type {
        RepoDetectionSessionType::Local => {
            Either::Left(DetectedRepositories::handle(ctx).update(ctx, |repos, ctx| {
                repos.detect_possible_git_repo(
                    active_directory,
                    source,
                    None::<Ready<Option<RemoteNavigationResult>>>,
                    ctx,
                )
            }))
        }
        RepoDetectionSessionType::Remote => Either::Right(ready(None)),
    }
}

/// Repository detection is not available in WASM builds because
/// `DetectedRepositories` is not registered there.
#[cfg(target_family = "wasm")]
pub fn detect_possible_git_repo<V: View>(
    _session_type: RepoDetectionSessionType,
    _active_directory: &str,
    _source: RepoDetectionSource,
    _ctx: &mut ViewContext<V>,
) -> impl Future<Output = Option<LocalOrRemotePath>> {
    ready(None)
}
