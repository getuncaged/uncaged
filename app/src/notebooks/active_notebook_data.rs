use uncaged_ui::{AppContext, Entity, ModelContext, SingletonEntity};

use super::CloudNotebookModel;
use crate::ai::document::ai_document_model::AIDocumentId;
use crate::cloud_object::breadcrumbs::ContainingObject;
use crate::cloud_object::model::persistence::{CloudModel, CloudModelEvent};
use crate::cloud_object::model::view::{CloudViewModel, Editor};
use crate::cloud_object::{CloudObject, Owner, Space};
use crate::drive::sharing::{ContentEditability, SharingAccessLevel};
use crate::notebooks::CloudNotebook;
use crate::server::cloud_objects::update_manager::{
    ObjectOperation, UpdateManager, UpdateManagerEvent,
};
use crate::server::ids::{ClientId, SyncId};

#[derive(Default, Clone)]
pub enum ActiveNotebook {
    #[default]
    None,
    // A notebook already stored in CloudModel, all relevant data should be queried
    // from CloudModel directly
    CommittedNotebook(SyncId),
    // A notebook that has been created and displayed in the view, but is not yet
    // committed to CloudModel
    NewNotebook(Box<CloudNotebook>),
}

#[derive(PartialEq, Eq, Default, Clone, Copy, Debug)]
pub enum Mode {
    #[default]
    Editing,
    View,
}

/// True if the object is currently being saved. We don't allow editing workflows
/// yet so this is only used for notebooks, but we will want it to apply for
/// workflows also.
#[derive(Default)]
pub enum SavingStatus {
    #[default]
    Saved,
    Saving,
}

/// Data displayed in the status bar that is also relevant for workflows and notebooks.
/// We share this data between views by making it a model.
#[derive(Default)]
pub struct ActiveNotebookData {
    /// Whether we're in editing, readonly or viewing mode.
    pub mode: Mode,
    pub saving_status: SavingStatus,
    pub active_notebook: ActiveNotebook,

    pub show_grab_edit_access_modal: bool,
}

impl ActiveNotebookData {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let update_manager = UpdateManager::handle(ctx);

        ctx.subscribe_to_model(&update_manager, |me, _, event, ctx| {
            me.handle_update_manager_event(event, ctx);
        });

        let cloud_model = CloudModel::handle(ctx);
        ctx.subscribe_to_model(&cloud_model, |me, _, event, ctx| {
            me.handle_cloud_model_event(event, ctx);
        });

        Self {
            ..Default::default()
        }
    }

    fn handle_cloud_model_event(&mut self, event: &CloudModelEvent, ctx: &mut ModelContext<Self>) {
        if let CloudModelEvent::ObjectMoved { type_and_id, .. } = event {
            if let Some(notebook_id) = type_and_id.as_notebook_id() {
                // Update breadcrumb when a notebook is moved, whether by the user or a
                // teammate.
                if self.is_active_notebook(notebook_id) {
                    ctx.emit(ActiveNotebookDataEvent::BreadcrumbsChanged);
                }
            }
        }
    }

    fn handle_update_manager_event(
        &mut self,
        event: &UpdateManagerEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        let UpdateManagerEvent::ObjectOperationComplete { result } = event;

        if result.id.is_none() || self.id() != result.id {
            return;
        }

        match &result.operation {
            ObjectOperation::Create { .. } => {
                if let Some(notebook_id) = result.id {
                    self.saving_status = SavingStatus::Saved;
                    self.active_notebook = ActiveNotebook::CommittedNotebook(notebook_id);
                    ctx.emit(ActiveNotebookDataEvent::BreadcrumbsChanged);
                    ctx.emit(ActiveNotebookDataEvent::Created);
                    ctx.notify();
                }
            }
            ObjectOperation::Update => {
                self.saving_status = SavingStatus::Saved;
                ctx.notify();
            }
            ObjectOperation::TakeEditAccess => {
                self.mode = Mode::Editing;
                ctx.emit(ActiveNotebookDataEvent::SwitchedToEditMode);
            }
            ObjectOperation::Trash | ObjectOperation::Untrash => {
                ctx.emit(ActiveNotebookDataEvent::TrashStatusChanged);
            }
            ObjectOperation::Delete { .. } | ObjectOperation::EmptyTrash => {}
        }
    }

    pub fn reset(&mut self) {
        self.mode = Mode::View;
        self.saving_status = SavingStatus::default();
        self.show_grab_edit_access_modal = false;
        self.active_notebook = ActiveNotebook::None;
    }

    pub fn open_new(
        &mut self,
        owner: Owner,
        initial_folder_id: Option<SyncId>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.reset();

        // create a new client id
        let new_id = ClientId::default();

        // Set the active notebook to be an uncommitted notebook
        self.active_notebook = ActiveNotebook::NewNotebook(Box::new(CloudNotebook::new_local(
            CloudNotebookModel::default(),
            owner,
            initial_folder_id,
            new_id,
        )));
        ctx.emit(ActiveNotebookDataEvent::BreadcrumbsChanged);
    }

    pub fn open_existing(&mut self, notebook_id: SyncId, ctx: &mut ModelContext<Self>) {
        self.reset();
        self.active_notebook = ActiveNotebook::CommittedNotebook(notebook_id);
        ctx.emit(ActiveNotebookDataEvent::BreadcrumbsChanged);
    }

    pub fn id(&self) -> Option<SyncId> {
        match &self.active_notebook {
            ActiveNotebook::None => None,
            ActiveNotebook::CommittedNotebook(id) => Some(*id),
            ActiveNotebook::NewNotebook(notebook) => Some(notebook.id),
        }
    }

    pub fn ai_document_id(&self, ctx: &AppContext) -> Option<AIDocumentId> {
        match &self.active_notebook {
            ActiveNotebook::None => None,
            ActiveNotebook::CommittedNotebook(id) => CloudModel::as_ref(ctx)
                .get_notebook(id)
                .and_then(|n| n.model().ai_document_id),
            ActiveNotebook::NewNotebook(notebook) => notebook.model().ai_document_id,
        }
    }

    pub fn active_notebook(&self) -> ActiveNotebook {
        self.active_notebook.clone()
    }

    /// Whether or not the notebook has been synced to the server.
    pub fn is_on_server(&self) -> bool {
        matches!(
            &self.active_notebook,
            ActiveNotebook::CommittedNotebook(SyncId::ServerId(_))
        )
    }

    /// Calculate the breadcrumbs for this object.
    pub fn breadcrumbs(&self, ctx: &AppContext) -> Option<Vec<ContainingObject>> {
        let cloud_notebook = match &self.active_notebook {
            ActiveNotebook::None => None,
            ActiveNotebook::CommittedNotebook(id) => CloudModel::as_ref(ctx).get_notebook(id),
            ActiveNotebook::NewNotebook(notebook) => Some(notebook.as_ref()),
        };

        cloud_notebook.map(|notebook| notebook.containing_objects_path(ctx))
    }

    /// The space that the active notebook is shown in for this user.
    pub fn space(&self, app: &AppContext) -> Option<Space> {
        match &self.active_notebook {
            ActiveNotebook::None => None,
            ActiveNotebook::CommittedNotebook(id) => CloudModel::as_ref(app)
                .get_notebook(id)
                .map(|notebook| notebook.space()),
            ActiveNotebook::NewNotebook(notebook) => Some(notebook.space()),
        }
    }

    /// The drive that owns the active notebook.
    pub fn owner(&self, app: &AppContext) -> Option<Owner> {
        match &self.active_notebook {
            ActiveNotebook::None => None,
            ActiveNotebook::CommittedNotebook(id) => CloudModel::as_ref(app)
                .get_notebook(id)
                .map(|notebook| notebook.permissions.owner),
            ActiveNotebook::NewNotebook(notebook) => Some(notebook.permissions.owner),
        }
    }

    pub fn is_active_notebook(&self, notebook_id: SyncId) -> bool {
        self.id() == Some(notebook_id)
    }

    /// Returns the current editor of the active object. Returns None
    /// if there is not currently an active notebook
    pub fn current_editor(&self, ctx: &AppContext) -> Option<Editor> {
        let id = self.id()?;
        CloudViewModel::as_ref(ctx).object_current_editor(&id.uid(), ctx)
    }

    /// Checks if this notebook is trashed or deleted.
    pub fn trash_status(&self, ctx: &AppContext) -> TrashStatus {
        match &self.active_notebook {
            ActiveNotebook::None | ActiveNotebook::NewNotebook(_) => TrashStatus::Active,
            ActiveNotebook::CommittedNotebook(id) => {
                let cloud_model = CloudModel::as_ref(ctx);
                match cloud_model.get_notebook(id) {
                    Some(notebook) => {
                        if notebook.is_trashed(cloud_model) {
                            TrashStatus::Trashed
                        } else {
                            TrashStatus::Active
                        }
                    }
                    None => TrashStatus::Deleted,
                }
            }
        }
    }

    /// The current user's access level on the notebook.
    pub fn access_level(&self, app: &AppContext) -> SharingAccessLevel {
        match &self.active_notebook {
            ActiveNotebook::CommittedNotebook(object_id) => {
                CloudViewModel::as_ref(app).access_level(&object_id.uid(), app)
            }
            ActiveNotebook::None | ActiveNotebook::NewNotebook(_) => SharingAccessLevel::Full,
        }
    }

    /// Whether or not the current user can edit the notebook.
    pub fn editability(&self, app: &AppContext) -> ContentEditability {
        match &self.active_notebook {
            ActiveNotebook::CommittedNotebook(object_id) => {
                CloudViewModel::as_ref(app).object_editability(&object_id.uid(), app)
            }
            ActiveNotebook::None | ActiveNotebook::NewNotebook(_) => ContentEditability::Editable,
        }
    }
}

pub enum ActiveNotebookDataEvent {
    /// The editing baton for the current object was successfully grabbed server-side.
    SwitchedToEditMode,
    /// The notebook's breadcrumbs were updated.
    BreadcrumbsChanged,
    /// This notebook was saved to Warp Drive for the first time.
    Created,
    /// This notebook was trashed or untrashed (used for refreshing pane overflow items)
    TrashStatusChanged,
}

/// Whether or not a notebook is trashed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrashStatus {
    Active,
    Trashed,
    Deleted,
}

impl TrashStatus {
    /// Whether or not the notebook can be edited in this state.
    pub fn is_editable(self) -> bool {
        match self {
            TrashStatus::Active => true,
            TrashStatus::Trashed | TrashStatus::Deleted => false,
        }
    }
}

impl Entity for ActiveNotebookData {
    type Event = ActiveNotebookDataEvent;
}
