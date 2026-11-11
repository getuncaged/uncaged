//! Wires the editor view (`uncaged_editor_view`) to the app services it cannot depend on:
//! settings, toasts, the workspace, the AI context menu and keybinding persistence.

use std::rc::Rc;

use uncaged_editor_view::host::{
    set_host, AiContextMenuHandle, AttachmentSink, CtrlCState, EditorHost, NextCommandSnapshot,
    NextCommandSource, OpaquePayload, ToastPersistence,
};
use uncaged_editor_view::{EditorView, Event as EditorEvent};
use uncaged_ui::keymap::Keystroke;
use uncaged_ui::{
    AppContext, EntityId, ModelHandle, SingletonEntity, ViewContext, ViewHandle, WindowId,
};

use crate::ai::agent::ImageContext;
use crate::ai::blocklist::{
    BlocklistAIContextModel, BlocklistAIHistoryModel, PendingAttachment, PendingFile,
};
use crate::ai::predict::next_command_model::NextCommandModel;
use crate::search::ai_context_menu::view::{AIContextMenu, AIContextMenuEvent};
use crate::settings::{AISettings, InputSettings};
use crate::suggestions::ignored_suggestions_model::{IgnoredSuggestionsModel, SuggestionType};
use crate::util::bindings::{reset_keybinding_to_default, set_custom_keybinding};
use crate::view_components::DismissibleToast;
use crate::workspace::{ToastStack, Workspace, WorkspaceAction};

/// Registers the app's [`EditorHost`]. Safe to call more than once.
pub fn register() {
    set_host(AppEditorHost);
}

struct AppEditorHost;

impl EditorHost for AppEditorHost {
    fn is_any_ai_enabled(&self, ctx: &AppContext) -> bool {
        AISettings::as_ref(ctx).is_any_ai_enabled(ctx)
    }

    fn is_universal_developer_input_enabled(&self, ctx: &AppContext) -> bool {
        InputSettings::as_ref(ctx).is_universal_developer_input_enabled(ctx)
    }

    fn at_context_menu_in_terminal_mode(&self, ctx: &AppContext) -> bool {
        *InputSettings::as_ref(ctx).at_context_menu_in_terminal_mode
    }

    fn ctrl_c_state(&self, ctx: &AppContext) -> CtrlCState {
        let terminal_view = ctx
            .windows()
            .active_window()
            .and_then(|active_window| {
                ctx
                    // Need to get the workspace info since we don't have access to the terminal view
                    // from the ClearBuffer action.
                    .views_of_type::<Workspace>(active_window)
                    .and_then(|views| views.first().cloned())
            })
            .and_then(|workspace| {
                workspace
                    .as_ref(ctx)
                    .active_tab_pane_group()
                    .as_ref(ctx)
                    .active_session_view(ctx)
            });

        // If an agent is responding, we don't want ctrl+c to clear the persistent input.
        let is_agent_responding = terminal_view
            .as_ref()
            .and_then(|terminal_view| {
                BlocklistAIHistoryModel::as_ref(ctx).active_conversation(terminal_view.id())
            })
            .is_some_and(|conversation| {
                conversation.status().is_in_progress() && conversation.exchange_count() > 0
            });

        // If there is a pending passive ai block, we don't want ctrl+c to clear the buffer.
        let is_pending_passive_ai_block = terminal_view.is_some_and(|terminal_view| {
            let terminal_model = terminal_view.as_ref(ctx).model.lock();
            terminal_model
                .block_list()
                .last_non_hidden_ai_block_handle(ctx)
                .is_some_and(|ai_block| {
                    let block = ai_block.as_ref(ctx);
                    // Ctrl+c should dismiss the passive ai block only if the keybindings for the block are not hidden.
                    let is_pending_code_diff = block.find_undismissed_code_diff(ctx).is_some();
                    let is_pending_suggested_prompt = block
                        .pending_unit_test_suggestion(ctx)
                        .is_some_and(|suggested_prompt| {
                            !suggested_prompt.as_ref(ctx).is_keybindings_hidden()
                        });
                    block.is_passive_conversation(ctx)
                        && (is_pending_code_diff || is_pending_suggested_prompt)
                })
        });

        CtrlCState {
            is_agent_responding,
            is_pending_passive_ai_block,
        }
    }

    fn show_error_toast(
        &self,
        message: String,
        persistence: ToastPersistence,
        window_id: WindowId,
        ctx: &mut AppContext,
    ) {
        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            let toast = DismissibleToast::error(message);
            match persistence {
                ToastPersistence::Ephemeral => {
                    toast_stack.add_ephemeral_toast(toast, window_id, ctx)
                }
                ToastPersistence::Persistent => {
                    toast_stack.add_persistent_toast(toast, window_id, ctx)
                }
            }
        });
    }

    fn is_shell_command_suggestion_ignored(&self, command: &str, ctx: &AppContext) -> bool {
        IgnoredSuggestionsModel::as_ref(ctx).is_ignored(command, SuggestionType::ShellCommand)
    }

    fn open_keybinding_settings(
        &self,
        binding_label: &str,
        window_id: WindowId,
        view_id: EntityId,
        ctx: &mut AppContext,
    ) {
        ctx.dispatch_typed_action_for_view(
            window_id,
            view_id,
            &WorkspaceAction::ConfigureKeybindingSettings {
                keybinding_name: Some(binding_label.to_owned()),
            },
        );
    }

    fn set_custom_keybinding(
        &self,
        binding_name: &str,
        keystroke: &Keystroke,
        ctx: &mut AppContext,
    ) {
        set_custom_keybinding(binding_name, keystroke, ctx);
    }

    fn reset_keybinding_to_default(&self, binding_name: &str, ctx: &mut AppContext) {
        reset_keybinding_to_default(binding_name, ctx);
    }

    fn create_ai_context_menu(
        &self,
        ctx: &mut ViewContext<EditorView>,
    ) -> Option<AiContextMenuHandle> {
        let ai_context_menu = ctx.add_typed_action_view(AIContextMenu::new);
        ctx.subscribe_to_view(
            &ai_context_menu,
            |_, _, event: &AIContextMenuEvent, ctx| match event {
                AIContextMenuEvent::Close => {
                    ctx.emit(EditorEvent::SetAIContextMenuOpen(false));
                    ctx.focus_self();
                    ctx.notify();
                }
                AIContextMenuEvent::ResultAccepted { action } => {
                    ctx.emit(EditorEvent::AcceptAIContextMenuItem(OpaquePayload::new(
                        action.clone(),
                    )));
                    ctx.focus_self();
                    ctx.notify();
                }
                AIContextMenuEvent::CategorySelected { category } => {
                    ctx.emit(EditorEvent::SelectAIContextMenuCategory(
                        OpaquePayload::new(*category),
                    ));
                    ctx.focus_self();
                    ctx.notify();
                }
            },
        );
        Some(AiContextMenuHandle::new(
            ai_context_menu,
            |menu: &AIContextMenu, ctx| menu.should_render(ctx),
        ))
    }
}

/// Gives [`EditorView`] typed access to the app's `@` context menu, which the editor crate only
/// knows as an opaque handle.
pub trait EditorViewAIContextMenuExt {
    fn ai_context_menu(&self) -> Option<&ViewHandle<AIContextMenu>>;
}

impl EditorViewAIContextMenuExt for EditorView {
    fn ai_context_menu(&self) -> Option<&ViewHandle<AIContextMenu>> {
        self.ai_context_menu_handle()
            .and_then(|handle| handle.view_handle())
    }
}

/// Lets an editor read and clear the next-command suggestion state held by `model`.
pub fn next_command_source(model: ModelHandle<NextCommandModel>) -> NextCommandSource {
    let snapshot_model = model.clone();
    NextCommandSource {
        snapshot: Rc::new(move |ctx| {
            let state = snapshot_model.as_ref(ctx).get_state();
            NextCommandSnapshot {
                is_cycling: state.is_cycling(),
                command_suggestion: state.command_suggestion().map(str::to_owned),
            }
        }),
        clear: Rc::new(move |ctx| {
            model.update(ctx, |model, _| {
                model.clear_state();
            });
        }),
    }
}

/// Lets an editor append the attachments it processes to the pending context in `model`.
pub fn attachment_sink(model: ModelHandle<BlocklistAIContextModel>) -> AttachmentSink {
    let images_model = model.clone();
    AttachmentSink {
        append_images: Rc::new(move |images, ctx| {
            let images = images
                .into_iter()
                .map(|image| ImageContext {
                    data: image.data,
                    mime_type: image.mime_type,
                    file_name: image.file_name,
                    is_figma: image.is_figma,
                })
                .collect();
            images_model.update(ctx, |context_model, ctx| {
                context_model.append_pending_images(images, ctx);
            });
        }),
        append_files: Rc::new(move |files, ctx| {
            let attachments = files
                .into_iter()
                .map(|file| {
                    PendingAttachment::File(PendingFile {
                        file_name: file.file_name,
                        file_path: file.file_path,
                        mime_type: file.mime_type,
                    })
                })
                .collect();
            model.update(ctx, |context_model, ctx| {
                context_model.append_pending_attachments(attachments, ctx);
            });
        }),
    }
}
