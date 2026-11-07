use std::collections::{HashMap, HashSet};

use warpui::{Entity, ModelContext, SingletonEntity};

use super::nodes::{self, FileId};
use crate::cloud_object::Owner;
use crate::notebooks::CloudNotebookModel;
use crate::server::cloud_objects::update_manager::{InitiatedBy, UpdateManager};
use crate::server::ids::{ClientId, ObjectUid, SyncId};
use crate::workflows::workflow::Workflow;
use crate::workflows::workflow_enum::WorkflowEnum;

pub(super) enum ImportQueueEvent {
    /// Every object parsed from the file was created. `uid` identifies one of them, to open.
    FileCompleted { file_id: FileId, uid: ObjectUid },
    FolderCompleted {
        folder_id: nodes::FolderId,
        uid: ObjectUid,
    },
}

#[derive(Debug)]
pub(super) enum ParentId {
    FolderToUpload(ClientId),
    InitialFolder(Option<SyncId>),
}

#[derive(Debug)]
pub(super) struct ImportQueueArgs {
    pub(super) owner: Owner,
    pub(super) parent_id: ParentId,
    pub(super) content: RequestContent,
}

#[derive(Debug)]
pub(super) enum RequestContent {
    Folder {
        name: String,
        client_id: ClientId,
        folder_id: nodes::FolderId,
    },
    Notebook {
        title: String,
        data: String,
        client_id: ClientId,
        file_id: FileId,
    },
    Workflow {
        workflows: Vec<(Workflow, ClientId)>,
        workflow_enums: HashMap<ClientId, WorkflowEnum>,
        file_id: FileId,
    },
}

/// Creates imported objects in Warp Drive. An object inside an imported folder waits until the
/// folder exists.
#[derive(Default)]
pub(super) struct ImportQueue {
    queue: Vec<ImportQueueArgs>,
    created_folders: HashSet<ClientId>,
}

impl ImportQueue {
    // Whether the folder an item goes into exists.
    fn parent_created(&self, item: &ImportQueueArgs) -> bool {
        match &item.parent_id {
            ParentId::FolderToUpload(id) => self.created_folders.contains(id),
            ParentId::InitialFolder(_) => true,
        }
    }

    // Enqueue a new request to the import queue.
    pub fn enqueue(&mut self, arg: ImportQueueArgs, ctx: &mut ModelContext<Self>) {
        self.queue.push(arg);
        self.dequeue(ctx);
    }

    // Create every queued item whose folder exists.
    fn dequeue(&mut self, ctx: &mut ModelContext<Self>) {
        while let Some(idx) = self.queue.iter().position(|item| self.parent_created(item)) {
            let dequeued_item = self.queue.remove(idx);
            let parent_id = match dequeued_item.parent_id {
                ParentId::FolderToUpload(client_id) => Some(SyncId::ClientId(client_id)),
                ParentId::InitialFolder(folder_id) => folder_id,
            };

            match dequeued_item.content {
                RequestContent::Folder {
                    name,
                    client_id,
                    folder_id,
                } => {
                    UpdateManager::handle(ctx).update(ctx, |update_manager, ctx| {
                        update_manager.create_folder(
                            name,
                            dequeued_item.owner,
                            client_id,
                            parent_id,
                            false,
                            InitiatedBy::User,
                            ctx,
                        );
                    });
                    self.created_folders.insert(client_id);
                    ctx.emit(ImportQueueEvent::FolderCompleted {
                        folder_id,
                        uid: SyncId::ClientId(client_id).uid(),
                    });
                }
                RequestContent::Notebook {
                    title,
                    data,
                    client_id,
                    file_id,
                } => {
                    UpdateManager::handle(ctx).update(ctx, |update_manager, ctx| {
                        update_manager.create_notebook(
                            client_id,
                            dequeued_item.owner,
                            parent_id,
                            CloudNotebookModel {
                                title,
                                data,
                                ai_document_id: None,
                                conversation_id: None,
                            },
                            false,
                            ctx,
                        );
                    });
                    ctx.emit(ImportQueueEvent::FileCompleted {
                        file_id,
                        uid: SyncId::ClientId(client_id).uid(),
                    });
                }
                RequestContent::Workflow {
                    workflows,
                    workflow_enums,
                    file_id,
                } => {
                    let last_workflow_id = workflows.last().map(|(_, client_id)| *client_id);
                    UpdateManager::handle(ctx).update(ctx, |update_manager, ctx| {
                        // Create any new workflow enums
                        for (client_id, workflow_enum) in workflow_enums {
                            update_manager.create_workflow_enum(
                                workflow_enum,
                                dequeued_item.owner,
                                client_id,
                                false,
                                ctx,
                            );
                        }

                        // Create the workflow
                        for (workflow, client_id) in workflows {
                            update_manager.create_workflow(
                                workflow,
                                dequeued_item.owner,
                                parent_id,
                                client_id,
                                false,
                                ctx,
                            );
                        }
                    });
                    if let Some(client_id) = last_workflow_id {
                        ctx.emit(ImportQueueEvent::FileCompleted {
                            file_id,
                            uid: SyncId::ClientId(client_id).uid(),
                        });
                    }
                }
            }
        }
    }
}

impl Entity for ImportQueue {
    type Event = ImportQueueEvent;
}
