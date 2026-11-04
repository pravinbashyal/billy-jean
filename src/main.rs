mod editor;
// Dev-only bridge to the `gpui-mcp` stdio inspection server. Compiled only with
// `--features gpui-mcp`.
#[cfg(feature = "gpui-mcp")]
mod inspect;

use std::borrow::Cow;

use billing::app_state::AppState;
use billing::fonts;
use billing::identity_store::{self, Identity};
use gpui::{App, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use gpui_component::{Root, Theme};
use gpui_platform::application;

/// Loads the persisted identity for pre-fill (design D5).
///
/// A missing file is a first run and yields an empty identity without error.
/// An unreadable or unsupported file is reported on stderr and the editor
/// starts empty rather than refusing to open.
fn load_identity() -> Identity {
    match identity_store::load() {
        Ok(identity) => identity,
        Err(e) => {
            eprintln!("warning: could not load saved identity, starting empty: {e}");
            Identity::default()
        }
    }
}

fn main() {
    application().run(|cx: &mut App| {
        // Register the bundled faces so later rendering can use them.
        cx.text_system()
            .add_fonts(vec![
                Cow::Borrowed(fonts::REGULAR),
                Cow::Borrowed(fonts::BOLD),
                Cow::Borrowed(fonts::SEMIBOLD),
            ])
            .expect("bundled fonts must be valid");

        // Initialize gpui-component (theme, global state, input/table primitives).
        gpui_component::init(cx);
        // Keyboard steps for the invoice date fields (task 4.3).
        editor::bind_date_field_keys(cx);
        // Full-screen preview keys: Cmd+Shift+F and, inside the overlay, Escape (task 5.2).
        editor::bind_preview_keys(cx);

        // Point the UI theme at the bundled font instead of the system UI font,
        // so every text element (and the gpui-component widgets) resolves it.
        Theme::update(cx, |theme| theme.font_family = fonts::FAMILY.into());

        let state = AppState::new(load_identity());
        // Edits to Sender and Bank are saved here after each change (task 7.2).
        let identity_dir = identity_store::default_config_dir();
        if identity_dir.is_none() {
            eprintln!("warning: no per-user config directory; identity will not be saved");
        }

        // Dev tooling: start the gpui-mcp inspection server before opening the
        // window so its address/token are printed up front. The editor handle is
        // captured from the window builder below to wire the action-drain loop.
        #[cfg(feature = "gpui-mcp")]
        let mcp = inspect::start();
        #[cfg(feature = "gpui-mcp")]
        let view_slot: std::rc::Rc<std::cell::RefCell<Option<gpui::Entity<editor::EditorView>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));

        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                {
                    #[cfg(feature = "gpui-mcp")]
                    let view_slot = view_slot.clone();
                    move |window, cx| {
                        let view = cx.new(|cx| {
                            editor::EditorView::new(state, window, cx)
                                .with_identity_dir(identity_dir)
                        });
                        #[cfg(feature = "gpui-mcp")]
                        view_slot.replace(Some(view.clone()));
                        cx.new(|cx| Root::new(view, window, cx))
                    }
                },
            )
            .expect("failed to open main window");

        // Dev tooling: hand the editor to the inspector and spawn the drain loop.
        #[cfg(feature = "gpui-mcp")]
        match mcp {
            Ok((inspector, actions)) => match view_slot.borrow().clone() {
                Some(view) => inspect::drive(view, window.into(), inspector, actions, cx),
                None => eprintln!("gpui-mcp: editor view was not captured; inspection disabled"),
            },
            Err(error) => eprintln!("gpui-mcp: {error}; inspection disabled"),
        }
        #[cfg(not(feature = "gpui-mcp"))]
        let _ = window;

        cx.activate(true);
    });
}
