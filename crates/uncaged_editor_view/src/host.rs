//! The seam between the editor view and the rest of the app.
//!
//! The editor view needs a handful of app-level facts and services (is AI enabled, show a toast,
//! look up the ignored-suggestions list, build the `@` context menu, ...) that live in modules
//! that depend on the editor themselves. Instead of reaching into them, the editor asks the
//! [`EditorHost`] the app registers once at startup via [`set_host`]. Until one is registered
//! (for example in this crate's own unit tests) every hook falls back to its default.

use std::any::Any;
use std::rc::Rc;
use std::sync::OnceLock;

use uncaged_ui::elements::{ChildView, Element};
use uncaged_ui::keymap::Keystroke;
use uncaged_ui::{AppContext, EntityId, View, ViewContext, ViewHandle, WindowId};

use crate::EditorView;

/// How a toast raised by the editor should be shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastPersistence {
    Ephemeral,
    Persistent,
}

/// Why `ctrl-c` must leave the input buffer alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CtrlCState {
    /// An agent is currently responding in the active session.
    pub is_agent_responding: bool,
    /// The active session has a pending passive AI block that `ctrl-c` should dismiss instead.
    pub is_pending_passive_ai_block: bool,
}

/// A snapshot of the "next command" suggestion state the editor needs to render and apply
/// suggestions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NextCommandSnapshot {
    pub is_cycling: bool,
    pub command_suggestion: Option<String>,
}

/// Where the editor reads the next-command suggestion state from and clears it.
#[derive(Clone)]
pub struct NextCommandSource {
    pub snapshot: Rc<dyn Fn(&AppContext) -> NextCommandSnapshot>,
    pub clear: Rc<dyn Fn(&mut AppContext)>,
}

/// A base64-encoded image attachment produced from pasted or dropped image data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessedImage {
    pub data: String,
    pub mime_type: String,
    pub file_name: String,
    pub is_figma: bool,
}

/// A non-image file attachment, stored by path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachedFile {
    pub file_name: String,
    pub file_path: std::path::PathBuf,
    pub mime_type: String,
}

pub type AppendImages = Rc<dyn Fn(Vec<ProcessedImage>, &mut AppContext)>;
pub type AppendFiles = Rc<dyn Fn(Vec<AttachedFile>, &mut AppContext)>;

/// Where the editor sends the attachments it processes.
#[derive(Clone)]
pub struct AttachmentSink {
    pub append_images: AppendImages,
    pub append_files: AppendFiles,
}

/// A value the app attaches to an editor event that the editor itself never looks inside.
#[derive(Clone)]
pub struct OpaquePayload(Rc<dyn Any>);

impl OpaquePayload {
    pub fn new<T: 'static>(value: T) -> Self {
        Self(Rc::new(value))
    }

    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}

impl std::fmt::Debug for OpaquePayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OpaquePayload(..)")
    }
}

/// The `@` context menu view the app builds for an editor, type-erased.
pub struct AiContextMenuHandle {
    handle: Box<dyn Any>,
    render: Box<dyn Fn() -> Box<dyn Element>>,
    should_render: Box<dyn Fn(&AppContext) -> bool>,
}

impl AiContextMenuHandle {
    pub fn new<V: View>(
        handle: ViewHandle<V>,
        should_render: impl Fn(&V, &AppContext) -> bool + 'static,
    ) -> Self {
        let render_handle = handle.clone();
        let should_render_handle = handle.clone();
        Self {
            handle: Box::new(handle),
            render: Box::new(move || ChildView::new(&render_handle).finish()),
            should_render: Box::new(move |ctx| {
                should_render(should_render_handle.as_ref(ctx), ctx)
            }),
        }
    }

    /// The menu view, if it is a `V`.
    pub fn view_handle<V: View>(&self) -> Option<&ViewHandle<V>> {
        self.handle.downcast_ref()
    }

    pub(crate) fn render(&self) -> Box<dyn Element> {
        (self.render)()
    }

    pub(crate) fn should_render(&self, ctx: &AppContext) -> bool {
        (self.should_render)(ctx)
    }
}

/// App-level facts and services the editor view relies on. Every hook has a conservative default
/// so the editor works standalone.
pub trait EditorHost: Send + Sync + 'static {
    /// Whether any AI feature is enabled.
    fn is_any_ai_enabled(&self, _ctx: &AppContext) -> bool {
        false
    }

    /// Whether the universal developer input is enabled.
    fn is_universal_developer_input_enabled(&self, _ctx: &AppContext) -> bool {
        false
    }

    /// Whether the `@` context menu may be opened from the terminal (non-AI) input mode.
    fn at_context_menu_in_terminal_mode(&self, _ctx: &AppContext) -> bool {
        false
    }

    /// What is happening in the active session that `ctrl-c` should respect.
    fn ctrl_c_state(&self, _ctx: &AppContext) -> CtrlCState {
        CtrlCState::default()
    }

    /// Shows an error toast in `window_id`.
    fn show_error_toast(
        &self,
        _message: String,
        _persistence: ToastPersistence,
        _window_id: WindowId,
        _ctx: &mut AppContext,
    ) {
    }

    /// Whether `command` was dismissed by the user as a next-command suggestion.
    fn is_shell_command_suggestion_ignored(&self, _command: &str, _ctx: &AppContext) -> bool {
        false
    }

    /// Opens the keybinding settings focused on `binding_label`, dispatched from `view_id`.
    fn open_keybinding_settings(
        &self,
        _binding_label: &str,
        _window_id: WindowId,
        _view_id: EntityId,
        _ctx: &mut AppContext,
    ) {
    }

    /// Persists a custom keybinding for an editable binding.
    fn set_custom_keybinding(
        &self,
        _binding_name: &str,
        _keystroke: &Keystroke,
        _ctx: &mut AppContext,
    ) {
    }

    /// Resets an editable binding to its default trigger, persisting the change.
    fn reset_keybinding_to_default(&self, _binding_name: &str, _ctx: &mut AppContext) {}

    /// Builds the `@` context menu for an editor created with `include_ai_context_menu`.
    fn create_ai_context_menu(
        &self,
        _ctx: &mut ViewContext<EditorView>,
    ) -> Option<AiContextMenuHandle> {
        None
    }
}

struct DefaultHost;

impl EditorHost for DefaultHost {}

static HOST: OnceLock<Box<dyn EditorHost>> = OnceLock::new();
static DEFAULT_HOST: DefaultHost = DefaultHost;

/// Registers the app's host. Only the first registration takes effect.
pub fn set_host(host: impl EditorHost) {
    let _ = HOST.set(Box::new(host));
}

pub(crate) fn host() -> &'static dyn EditorHost {
    match HOST.get() {
        Some(host) => host.as_ref(),
        None => &DEFAULT_HOST,
    }
}
