//! Dev-only bridge to the `gpui-mcp` stdio server (crates.io `gpui-mcp`).
//!
//! Compiled only with `--features gpui-mcp`. `gpui-mcp` is framework-neutral: it
//! never touches GPUI itself. This module is the adapter. It
//!
//! 1. starts an authenticated loopback [`InspectionServer`] and prints the
//!    address and token an MCP client passes to its `attach` tool,
//! 2. lets [`crate::editor::EditorView`] publish a semantic snapshot of the form
//!    on every render (see [`EditorView::inspect_publish`]), and
//! 3. drains the server's bounded action channel on the GPUI foreground executor,
//!    executing each [`InspectionAction`] against the window and replying.
//!
//! Only the executor side lives here; the per-node mapping and the actual
//! mutations live on `EditorView` so they reach its private inputs and state.

use std::cell::Cell;

use async_channel::{Receiver, Sender};
use gpui::{AnyWindowHandle, App, AppContext, Entity};
pub use gpui_mcp::{
    ActionRequest, InspectionAction, InspectionActionResult, InspectionRegistry, InspectionServer,
    SemanticBounds, SemanticNode, SemanticTree,
};

use crate::editor::EditorView;

/// Bounded like the crate's own example; actions are serialized on one executor
/// so a small queue is plenty.
const ACTION_QUEUE: usize = 16;

/// The app-side handle the editor publishes semantic snapshots through.
///
/// Holds the latest-value registry the server reads and a monotonic revision
/// counter. The [`InspectionServer`] is kept alive here; its listener runs on a
/// detached thread, but keeping the handle makes the ownership explicit.
pub struct Inspector {
    registry: InspectionRegistry,
    revision: Cell<u64>,
    _server: InspectionServer,
}

impl Inspector {
    /// Publishes a fresh snapshot, assigning the next revision. Cheap: the
    /// registry keeps only the latest tree and never blocks the UI.
    pub fn publish(&self, nodes: Vec<SemanticNode>) {
        let revision = self.revision.get() + 1;
        self.revision.set(revision);
        self.registry.publish(SemanticTree { revision, nodes });
    }
}

/// Convenience constructor for one semantic node. `bounds` is left unset: clients
/// are told to prefer stable ids over coordinates, and only the application root
/// needs real bounds (set by the caller for resize/screenshot settling).
pub fn node(
    id: impl Into<String>,
    role: &str,
    label: impl Into<String>,
    value: Option<String>,
    actions: &[&str],
) -> SemanticNode {
    SemanticNode {
        id: id.into(),
        role: role.to_owned(),
        label: label.into(),
        value,
        bounds: None,
        focused: false,
        actions: actions.iter().map(|a| (*a).to_owned()).collect(),
    }
}

/// Starts the loopback inspection server on an ephemeral port.
///
/// Returns the [`Inspector`] the editor publishes through and the receiver the
/// drain loop reads. The address and token are printed to stderr so a human can
/// hand them to the MCP client's `attach` tool.
pub fn start() -> Result<(Inspector, Receiver<ActionRequest>), String> {
    let registry = InspectionRegistry::default();
    let (action_tx, action_rx): (Sender<ActionRequest>, Receiver<ActionRequest>) =
        async_channel::bounded(ACTION_QUEUE);
    let server = InspectionServer::start(0, registry.clone(), action_tx)?;

    eprintln!("gpui-mcp inspection address: {}", server.address());
    eprintln!("gpui-mcp inspection token:   {}", server.token());
    eprintln!("gpui-mcp: call the server's `attach` tool with the address and token above.");

    let inspector = Inspector {
        registry,
        revision: Cell::new(0),
        _server: server,
    };
    Ok((inspector, action_rx))
}

/// Attaches the inspector to the editor and spawns the action-drain loop.
///
/// The loop receives one [`ActionRequest`] at a time, runs it on the window's
/// foreground executor via [`EditorView::inspect_execute`], and replies through
/// the request's response channel. A closed channel (server gone) ends the loop.
pub fn drive(
    view: Entity<EditorView>,
    window: AnyWindowHandle,
    inspector: Inspector,
    actions: Receiver<ActionRequest>,
    cx: &mut App,
) {
    // Hand the inspector to the editor so its next render publishes a snapshot.
    let attach = view.clone();
    cx.update_window(window, |_, _window, cx| {
        attach.update(cx, |view, _cx| view.attach_inspector(inspector));
    })
    .ok();

    cx.spawn(async move |cx| {
        while let Ok(request) = actions.recv().await {
            let ActionRequest { action, response } = request;
            // No `view.update` here: the action runs with the window but without a
            // lease on the editor, so GPUI may draw it mid-action (see `inspect_execute`).
            let outcome = cx.update_window(window, |_, window, cx| {
                EditorView::inspect_execute(&view, action, window, cx)
            });
            let reply = match outcome {
                Ok(result) => result,
                Err(error) => Err(format!("inspection window update failed: {error:#}")),
            };
            // The server side may have already timed out; a dropped receiver is fine.
            let _ = response.send(reply);
        }
    })
    .detach();
}

/// Drives actions through the real [`drive`] drain loop, not by calling
/// `EditorView::inspect_execute` directly, so lease conflicts show up here.
#[cfg(all(test, feature = "gpui-mcp"))]
mod drain_tests {
    use super::*;
    use billing::app_state::AppState;
    use billing::identity_store::Identity;
    use gpui::{TestAppContext, VisualContext, VisualTestContext};
    use gpui_mcp::{NormalizedModifiers, SemanticPoint};
    use std::sync::mpsc;

    type Reply = mpsc::Receiver<Result<InspectionActionResult, String>>;

    fn request(action: InspectionAction) -> (ActionRequest, Reply) {
        let (response, reply) = mpsc::sync_channel(1);
        (ActionRequest { action, response }, reply)
    }

    fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        cx.update(gpui_component::init);
        cx.add_window_view(|window, cx| {
            EditorView::new(AppState::new(Identity::default()), window, cx)
        })
    }

    /// `Focus` leaves the window dirty; the following `Key` then makes GPUI redraw
    /// while dispatching. Before the fix this panicked with "already being updated".
    #[gpui::test]
    fn focus_then_key_through_drain_loop_does_not_panic(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        let window = cx.window_handle();
        let (inspector, _server_actions) = start().expect("inspection server starts");
        let (actions_tx, actions_rx) = async_channel::bounded(ACTION_QUEUE);
        cx.update(|_window, cx| drive(view.clone(), window, inspector, actions_rx, cx));

        let (focus, focus_reply) = request(InspectionAction::Focus {
            id: "sender.name".to_owned(),
        });
        actions_tx.try_send(focus).expect("queue focus");
        cx.run_until_parked();
        assert_eq!(
            focus_reply.try_recv(),
            Ok(Ok(InspectionActionResult::Complete))
        );

        let (key, key_reply) = request(InspectionAction::Key {
            key: "a".to_owned(),
            modifiers: NormalizedModifiers::default(),
        });
        actions_tx.try_send(key).expect("queue key");
        cx.run_until_parked();
        assert_eq!(key_reply.try_recv(), Ok(Ok(InspectionActionResult::Complete)));

        // Scroll and Drag dispatch mouse events, which can also redraw the window.
        let (scroll, scroll_reply) = request(InspectionAction::Scroll {
            position: SemanticPoint { x: 100.0, y: 100.0 },
            delta_x: 0.0,
            delta_y: -20.0,
            modifiers: NormalizedModifiers::default(),
        });
        actions_tx.try_send(scroll).expect("queue scroll");
        cx.run_until_parked();
        assert_eq!(scroll_reply.try_recv(), Ok(Ok(InspectionActionResult::Complete)));

        let (drag, drag_reply) = request(InspectionAction::Drag {
            from: SemanticPoint { x: 100.0, y: 100.0 },
            to: SemanticPoint { x: 120.0, y: 110.0 },
        });
        actions_tx.try_send(drag).expect("queue drag");
        cx.run_until_parked();
        assert_eq!(drag_reply.try_recv(), Ok(Ok(InspectionActionResult::Complete)));
    }
}
