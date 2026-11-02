//! Keeps the configured windowed size and aspect ratio inside the monitor's work area.

use std::sync::{Arc, Mutex};

use tauri::utils::config::WindowConfig;
use tauri::{LogicalSize, PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent};

#[derive(Clone, Copy, Debug, PartialEq)]
struct DisplayArea {
    origin: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    scale: f64,
}

#[derive(Clone, Copy, Debug)]
struct Geometry {
    position: PhysicalPosition<i32>,
    inner: PhysicalSize<u32>,
    outer: PhysicalSize<u32>,
}

#[derive(Debug, PartialEq)]
struct Placement {
    inner: PhysicalSize<u32>,
    position: PhysicalPosition<i32>,
}

#[derive(Debug)]
struct Adaptation {
    display: DisplayArea,
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
        scale: monitor.scale_factor(),
    };
    if display.size.width == 0
        || display.size.height == 0
        || !display.scale.is_finite()
        || display.scale <= 0.0
    {
        return Err("The display reported an invalid work area or scale factor".into());
    }
    Ok(display)
}

fn geometry(window: &WebviewWindow) -> Result<Geometry, String> {
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

fn placement(
    display: DisplayArea,
    preferred: LogicalSize<f64>,
    current: Geometry,
    center: bool,
) -> Placement {
    // Preserve the configured logical size on roomy displays, and shrink both
    // axes together when necessary. Never inherit a snapped or user-sized shape.
    // Work areas include taskbars; frame extents are physical pixels too.
    let frame = PhysicalSize::new(
        current.outer.width.saturating_sub(current.inner.width),
        current.outer.height.saturating_sub(current.inner.height),
    );
    let available = PhysicalSize::new(
        display.size.width.saturating_sub(frame.width).max(1),
        display.size.height.saturating_sub(frame.height).max(1),
    );
    let scale = display
        .scale
        .min(f64::from(available.width) / preferred.width)
        .min(f64::from(available.height) / preferred.height);
    let inner = PhysicalSize::new(
        ((preferred.width * scale).round() as u32).clamp(1, available.width),
        ((preferred.height * scale).round() as u32).clamp(1, available.height),
    );
    let outer = PhysicalSize::new(
        inner.width.saturating_add(frame.width),
        inner.height.saturating_add(frame.height),
    );
    let coordinate = |origin: i32, extent: u32, size: u32, current: i32| {
        let start = i64::from(origin);
        let remaining = i64::from(extent.saturating_sub(size));
        let position = if center {
            start + remaining / 2
        } else {
            i64::from(current).clamp(start, start + remaining)
        };
        position.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    };
    Placement {
        inner,
        position: PhysicalPosition::new(
            coordinate(
                display.origin.x,
                display.size.width,
                outer.width,
                current.position.x,
            ),
            coordinate(
                display.origin.y,
                display.size.height,
                outer.height,
                current.position.y,
            ),
        ),
    }
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
    let current = geometry(window)?;
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
    disable_default_context_menus(window)?;
    let display = display_area(window)?;
    let fitted = apply(window, display, preferred, true)?;
    let state = Arc::new(Mutex::new(Adaptation {
        display,
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
            {
                let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
                if !state.needs_fit(display, matches!(event, WindowEvent::Resized(_))) {
                    return Ok(());
                }
                // Native setters can synchronously deliver more window events.
                // Never hold this lock while calling them or refit recursively.
                state.applying = true;
                state.pending = true;
            }
            let result = apply(&target, display, preferred, false);
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

    fn preferred() -> LogicalSize<f64> {
        LogicalSize::new(1560.0, 900.0)
    }

    fn display(width: u32, height: u32, scale: f64) -> DisplayArea {
        DisplayArea {
            origin: PhysicalPosition::new(0, 0),
            size: PhysicalSize::new(width, height),
            scale,
        }
    }

    fn window(width: u32, height: u32) -> Geometry {
        Geometry {
            position: PhysicalPosition::new(0, 0),
            inner: PhysicalSize::new(width, height),
            outer: PhysicalSize::new(width, height),
        }
    }

    #[test]
    fn normal_desktop_centers_the_default_without_changing_its_size() {
        let fit = placement(
            display(1920, 1040, 1.0),
            preferred(),
            window(1560, 900),
            true,
        );
        assert_eq!(fit.inner, PhysicalSize::new(1560, 900));
        assert_eq!(fit.position, PhysicalPosition::new(180, 70));
    }

    #[test]
    fn high_dpi_shrinks_both_axes_to_preserve_the_windowed_aspect_ratio() {
        let fit = placement(
            display(1920, 1040, 1.5),
            preferred(),
            window(2340, 1350),
            true,
        );
        assert_eq!(fit.inner, PhysicalSize::new(1803, 1040));
        assert_eq!(fit.position, PhysicalPosition::new(58, 0));
    }

    #[test]
    fn small_work_area_accounts_for_the_native_frame_without_stretching() {
        let mut current = window(1560, 900);
        current.outer = PhysicalSize::new(1576, 939);
        let fit = placement(display(1280, 720, 2.0), preferred(), current, true);
        assert_eq!(fit.inner, PhysicalSize::new(1180, 681));
        assert_eq!(fit.position, PhysicalPosition::new(42, 0));
    }

    #[test]
    fn monitor_change_restores_fixed_size_and_clamps_negative_coordinates() {
        let mut secondary = display(1920, 1040, 1.0);
        secondary.origin = PhysicalPosition::new(-1920, 40);
        let mut current = window(1100, 700);
        current.position = PhysicalPosition::new(-400, -200);
        let fit = placement(secondary, preferred(), current, false);
        assert_eq!(fit.inner, PhysicalSize::new(1560, 900));
        assert_eq!(fit.position, PhysicalPosition::new(-1560, 40));
    }

    #[test]
    fn returning_to_a_larger_monitor_recovers_the_configured_size() {
        let mut current = window(1000, 640);
        current.position = PhysicalPosition::new(100, 80);
        let fit = placement(display(3840, 2080, 1.0), preferred(), current, false);
        assert_eq!(fit.inner, PhysicalSize::new(1560, 900));
        assert_eq!(fit.position, current.position);
    }

    #[test]
    fn logical_size_is_consistent_across_system_scale_factors() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let area = display((1920.0 * scale) as u32, (1040.0 * scale) as u32, scale);
            let fit = placement(area, preferred(), window(1000, 700), true);
            assert_eq!(fit.inner.to_logical::<f64>(scale), preferred());
        }
    }

    #[test]
    fn arbitrary_resize_and_snap_sizes_cannot_change_the_windowed_shape() {
        for size in [(700, 900), (1920, 1040), (960, 1040), (1200, 700)] {
            let fit = placement(
                display(1920, 1040, 1.0),
                preferred(),
                window(size.0, size.1),
                false,
            );
            assert_eq!(fit.inner, PhysicalSize::new(1560, 900));
        }
    }

    #[test]
    fn fitted_window_keeps_its_ratio_and_stays_in_portrait_and_landscape_work_areas() {
        for (width, height) in [
            (1280, 680),
            (1366, 728),
            (1920, 1040),
            (1080, 1880),
            (3840, 2080),
        ] {
            for scale in [1.0, 1.25, 1.5, 2.0] {
                let fit = placement(
                    display(width, height, scale),
                    preferred(),
                    window(1560, 900),
                    true,
                );
                assert!(fit.position.x >= 0 && fit.position.y >= 0);
                assert!(fit.position.x as u32 + fit.inner.width <= width);
                assert!(fit.position.y as u32 + fit.inner.height <= height);
                let ratio_error = (f64::from(fit.inner.width)
                    - f64::from(fit.inner.height) * 1560.0 / 900.0)
                    .abs();
                assert!(
                    ratio_error <= 1.0,
                    "aspect ratio rounding exceeded one pixel: {fit:?}"
                );
            }
        }
    }

    #[test]
    fn every_resize_refits_but_ordinary_moves_do_not_and_setters_cannot_reenter() {
        let display = display(1920, 1040, 1.0);
        let mut state = Adaptation {
            display,
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
        assert_eq!(LogicalSize::new(main.width, main.height), preferred());
        assert!(main.min_width.is_none() && main.min_height.is_none());
        assert!(main.max_width.is_none() && main.max_height.is_none());
    }
}
