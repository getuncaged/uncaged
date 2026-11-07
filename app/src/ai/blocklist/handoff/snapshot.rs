//! Snapshot upload pipeline for local-to-cloud handoff.
//!
//! Owns the async upload orchestration that was previously inlined in
//! `Workspace::spawn_handoff_snapshot_upload`. Sessions flow through
//! [`spawn_handoff_snapshot_upload`], which uploads via [`SnapshotUploadTarget`]
//! and converges on [`settle_handoff_snapshot_result`] to update the model.

use std::path::PathBuf;
use std::sync::Arc;

use warp_util::standardized_path::StandardizedPath;
use warpui::{ModelHandle, SingletonEntity, ViewContext};

use crate::ai::agent_sdk::driver::upload_snapshot_for_handoff;
use crate::ai::blocklist::handoff::touched_repos::{derive_touched_workspace, TouchedWorkspace};
use crate::server::server_api::ai::{AIClient, InitialSnapshotToken};
use crate::server::server_api::ServerApiProvider;
use crate::terminal::view::ambient_agent::{AmbientAgentViewModel, SnapshotUploadStatus};
use crate::workspace::Workspace;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The outcome of a successful handoff snapshot upload.
///
/// Maps 1:1 to the two success variants in `UploadHandoffSnapshotResponse`:
/// either the server minted a token, or the workspace was empty (no files to
/// upload).
pub(crate) enum HandoffUploadResult {
    /// The upload succeeded and the server returned a snapshot token.
    Uploaded(InitialSnapshotToken),
    /// The workspace had no files to upload (no repos, no orphans).
    EmptyWorkspace,
}

/// Clients the snapshot upload runs with.
pub(crate) enum SnapshotUploadTarget {
    /// Run `derive_touched_workspace` + `upload_snapshot_for_handoff` locally.
    Local {
        ai_client: Arc<dyn AIClient>,
        http: Arc<http_client::Client>,
    },
}

// ---------------------------------------------------------------------------
// Upload pipeline
// ---------------------------------------------------------------------------

/// Spawns the async snapshot upload pipeline for a handoff pane.
///
/// Derives the touched workspace from `paths`, uploads repo patches + orphan
/// files, and settles the snapshot status on the model. Shared by both the
/// conversation-fork and fresh-launch handoff paths.
pub(crate) fn spawn_handoff_snapshot_upload(
    paths: Vec<StandardizedPath>,
    target: SnapshotUploadTarget,
    model_handle: ModelHandle<AmbientAgentViewModel>,
    ctx: &mut ViewContext<Workspace>,
) {
    ctx.spawn(
        upload_handoff_snapshot(paths, target),
        move |_workspace, (derived_workspace, upload_result), ctx| {
            model_handle.update(ctx, |model, model_ctx| {
                if !model.is_local_to_cloud_handoff() {
                    return;
                }
                model.set_pending_handoff_workspace(derived_workspace, model_ctx);
                settle_handoff_snapshot_result(model, upload_result, model_ctx);
            });
            maybe_auto_submit_handoff(&model_handle, ctx);
        },
    );
}

/// Shared async upload function.
///
/// Returns the derived workspace and the upload result.
async fn upload_handoff_snapshot(
    paths: Vec<StandardizedPath>,
    target: SnapshotUploadTarget,
) -> (TouchedWorkspace, Result<HandoffUploadResult, anyhow::Error>) {
    match target {
        SnapshotUploadTarget::Local { ai_client, http } => {
            let local_paths: Vec<PathBuf> =
                paths.iter().map(|sp| sp.to_local_path_lossy()).collect();
            let workspace = derive_touched_workspace(local_paths).await;
            let repo_paths: Vec<_> = workspace.repos.iter().map(|r| r.git_root.clone()).collect();
            let upload_result = upload_snapshot_for_handoff(
                repo_paths,
                workspace.orphan_files.clone(),
                ai_client,
                http.as_ref(),
            )
            .await;
            let result = match upload_result {
                Ok(Some(token)) => Ok(HandoffUploadResult::Uploaded(token)),
                Ok(None) => Ok(HandoffUploadResult::EmptyWorkspace),
                Err(e) => Err(e),
            };
            (workspace, result)
        }
    }
}

/// Resolve the upload target.
pub(crate) fn resolve_upload_target(ctx: &mut ViewContext<Workspace>) -> SnapshotUploadTarget {
    let server_api_provider = ServerApiProvider::as_ref(ctx);
    SnapshotUploadTarget::Local {
        ai_client: server_api_provider.get_ai_client(),
        http: server_api_provider.get_http_client(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Settle the snapshot upload status on the model from an upload result.
fn settle_handoff_snapshot_result(
    model: &mut AmbientAgentViewModel,
    result: Result<HandoffUploadResult, anyhow::Error>,
    model_ctx: &mut warpui::ModelContext<AmbientAgentViewModel>,
) {
    match result {
        Ok(HandoffUploadResult::Uploaded(token)) => {
            model.set_pending_handoff_snapshot_upload(
                SnapshotUploadStatus::Uploaded(token),
                model_ctx,
            );
        }
        Ok(HandoffUploadResult::EmptyWorkspace) => {
            model.set_pending_handoff_snapshot_upload(
                SnapshotUploadStatus::SkippedEmptyWorkspace,
                model_ctx,
            );
        }
        Err(err) => {
            log::warn!("Handoff snapshot upload failed: {err:#}");
            model.record_handoff_snapshot_upload_failed(format!("{err}"), model_ctx);
        }
    }
}

/// If the handoff model has a queued auto-submit payload, submit it now.
fn maybe_auto_submit_handoff(
    model_handle: &ModelHandle<AmbientAgentViewModel>,
    ctx: &mut ViewContext<Workspace>,
) {
    let launch = model_handle.update(ctx, |model, ctx| model.maybe_auto_submit_handoff(ctx));
    let Some(launch) = launch else {
        return;
    };
    model_handle.update(ctx, |model, ctx| {
        model.submit_handoff(launch.prompt, launch.attachments.request_attachments, ctx);
    });
}
