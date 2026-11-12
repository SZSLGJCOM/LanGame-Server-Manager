//! Automatically matches window size and content density to the current display.

use std::sync::{Arc, Mutex};

use tauri::utils::config::WindowConfig;
use tauri::{LogicalSize, WebviewWindow, WindowEvent};

pub(crate) mod drag;
mod geometry;
#[cfg(windows)]
mod snap;
#[cfg(all(windows, feature = "desktop-reliability"))]
pub(crate) use drag::{DragAnchor, restore_for_drag};
#[cfg(all(windows, feature = "desktop-reliability"))]
pub(crate) use snap::maximize_at_top_edge;

use geometry::{DisplayArea, Geometry, placement, zoom_factor};

#[derive(Debug)]
struct Adaptation {
    display: DisplayArea,
    zoom: f64,
    pending: bool,
    applying: bool,
    last_error: Option<String>,
}

impl Adaptation {
    fn needs_fit(&self, display: DisplayArea, resized: bool) -> bool {
        !self.applying && (resized || self.pending || self.display != display)
    }
}

fn display_area(window: &WebviewWindow) -> Result<DisplayArea, String> {
    let current = window
        .current_monitor()
        .map_err(|error| format!("read current monitor: {error}"))?;
    let monitor = match current {
        Some(monitor) => monitor,
        None => window
            .primary_monitor()
            .map_err(|error| format!("read primary monitor: {error}"))?
            .ok_or("No display is available for the desktop window")?,
    };
    let area = monitor.work_area();
    let display = DisplayArea {
        origin: area.position,
        size: area.size,
        screen: *monitor.size(),
        scale: monitor.scale_factor(),
    };
    if display.size.width == 0
        || display.size.height == 0
        || display.screen.width == 0
        || display.screen.height == 0
        || !display.scale.is_finite()
        || display.scale <= 0.0
    {
        return Err("The display reported an invalid work area or scale factor".into());
    }
    Ok(display)
}

fn window_geometry(window: &WebviewWindow) -> Result<Geometry, String> {
    Ok(Geometry {
        position: window
            .outer_position()
            .map_err(|error| format!("read window position: {error}"))?,
        inner: window
            .inner_size()
            .map_err(|error| format!("read window client size: {error}"))?,
        outer: window
            .outer_size()
            .map_err(|error| format!("read window outer size: {error}"))?,
    })
}

fn apply(
    window: &WebviewWindow,
    display: DisplayArea,
    preferred: LogicalSize<f64>,
    center: bool,
) -> Result<bool, String> {
    if window
        .is_minimized()
        .map_err(|error| format!("read minimized state: {error}"))?
        || window
            .is_maximized()
            .map_err(|error| format!("read maximized state: {error}"))?
        || window
            .is_fullscreen()
            .map_err(|error| format!("read fullscreen state: {error}"))?
    {
        // Let the OS own maximized/fullscreen bounds. Refit after restoration.
        return Ok(false);
    }
    let current = window_geometry(window)?;
    let target = placement(display, preferred, current, center);
    if current.inner != target.inner {
        window
            .set_size(target.inner)
            .map_err(|error| format!("fit desktop size to work area: {error}"))?;
    }
    if current.position != target.position {
        window
            .set_position(target.position)
            .map_err(|error| format!("fit desktop position to work area: {error}"))?;
    }
    Ok(true)
}

#[cfg(windows)]
fn disable_default_context_menus(window: &WebviewWindow) -> Result<(), String> {
    let (reply, response) = std::sync::mpsc::sync_channel(1);
    window
        .with_webview(move |platform| {
            // SAFETY: with_webview runs on the WebView2 STA. Disabling the
            // browser menu leaves DOM contextmenu handlers and edit shortcuts intact.
            let result = unsafe {
                platform
                    .controller()
                    .CoreWebView2()
                    .and_then(|core| core.Settings())
                    .and_then(|settings| settings.SetAreDefaultContextMenusEnabled(false))
            }
            .map_err(|error| format!("disable WebView2 default context menus: {error}"));
            let _ = reply.send(result);
        })
        .map_err(|error| format!("configure desktop WebView2 context menus: {error}"))?;
    // All prepare callers run on the UI thread, where Wry executes the closure
    // immediately. Propagate native failures without blocking the event loop.
    response.try_recv().map_err(|error| {
        format!("desktop WebView2 preparation must complete on the UI thread: {error}")
    })?
}

/// Called on the UI thread for every newly created window, including WebView recovery.
/// The caller keeps the window hidden until its initial placement is ready.
pub(crate) fn prepare(window: &WebviewWindow, config: &WindowConfig) -> Result<(), String> {
    let preferred = LogicalSize::new(config.width, config.height);
    if !preferred.width.is_finite()
        || !preferred.height.is_finite()
        || preferred.width <= 0.0
        || preferred.height <= 0.0
    {
        return Err("The configured desktop window size must be finite and positive".into());
    }
    // Keep this invariant on recovery and in native fixture windows as well.
    // The custom title bar can still explicitly maximize and restore the window.
    window
        .set_resizable(false)
        .map_err(|error| format!("disable desktop window resizing: {error}"))?;
    #[cfg(windows)]
    {
        snap::install(window)?;
        disable_default_context_menus(window)?;
    }
    let display = display_area(window)?;
    let zoom = zoom_factor(display);
    window
        .set_zoom(zoom)
        .map_err(|error| format!("adapt desktop content to display: {error}"))?;
    let fitted = apply(window, display, preferred, true)?;
    let state = Arc::new(Mutex::new(Adaptation {
        display,
        zoom,
        pending: !fitted,
        applying: false,
        last_error: None,
    }));
    let target = window.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::ScaleFactorChanged { .. }) {
            // Tao applies Windows' DPI rectangle after this callback. Wait for
            // its subsequent move/resize event so our bounds cannot be undone.
            state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .pending = true;
            return;
        }
        if !matches!(
            event,
            WindowEvent::Moved(_) | WindowEvent::Resized(_) | WindowEvent::Focused(true)
        ) {
            return;
        }
        let result = (|| {
            let display = display_area(&target)?;
            let zoom = zoom_factor(display);
            let update_zoom = {
                let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
                if !state.needs_fit(display, matches!(event, WindowEvent::Resized(_))) {
                    return Ok(());
                }
                // Native setters can synchronously deliver more window events.
                // Never hold this lock while calling them or refit recursively.
                state.applying = true;
                state.pending = true;
                state.zoom != zoom
            };
            let result = (|| {
                // Content density belongs to the display even while the OS owns
                // maximized/fullscreen bounds. Cache success independently of
                // placement so resize events cannot repeatedly reset page zoom.
                if update_zoom {
                    target
                        .set_zoom(zoom)
                        .map_err(|error| format!("adapt desktop content to display: {error}"))?;
                    state.lock().unwrap_or_else(|error| error.into_inner()).zoom = zoom;
                }
                apply(&target, display, preferred, false)
            })();
            let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
            state.applying = false;
            if let Ok(fitted) = &result {
                state.display = display;
                state.pending = !fitted;
            }
            result.map(|_| ())
        })();
        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
        match result {
            Ok(()) => state.last_error = None,
            Err(error) => {
                if state.last_error.as_ref() != Some(&error) {
                    eprintln!("adapt desktop window: {error}");
                }
                state.last_error = Some(error);
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::{PhysicalPosition, PhysicalSize};

    fn display() -> DisplayArea {
        DisplayArea {
            origin: PhysicalPosition::new(0, 0),
            size: PhysicalSize::new(2560, 1392),
            screen: PhysicalSize::new(2560, 1440),
            scale: 1.0,
        }
    }

    #[test]
    fn every_resize_refits_but_ordinary_moves_do_not_and_setters_cannot_reenter() {
        let display = display();
        let mut state = Adaptation {
            display,
            zoom: 1.0,
            pending: false,
            applying: false,
            last_error: None,
        };
        assert!(!state.needs_fit(display, false));
        // Includes restoring from maximized/fullscreen and OS snap attempts.
        assert!(state.needs_fit(display, true));
        state.pending = true;
        assert!(state.needs_fit(display, false));
        state.applying = true;
        assert!(!state.needs_fit(display, true));
        state.applying = false;
        state.pending = false;
        assert!(state.needs_fit(
            DisplayArea {
                scale: 1.25,
                ..display
            },
            false
        ));
        assert!(state.needs_fit(
            DisplayArea {
                screen: PhysicalSize::new(3840, 2160),
                ..display
            },
            false
        ));
    }

    #[test]
    fn desktop_configuration_disables_resize_without_limiting_maximized_bounds() {
        let config: tauri::utils::config::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let main = config
            .app
            .windows
            .iter()
            .find(|window| window.label == "main")
            .unwrap();
        assert!(!main.resizable);
        assert!(main.maximizable);
        assert_eq!(
            LogicalSize::new(main.width, main.height),
            LogicalSize::new(1560.0, 900.0)
        );
        assert!(main.min_width.is_none() && main.min_height.is_none());
        assert!(main.max_width.is_none() && main.max_height.is_none());
    }
}
