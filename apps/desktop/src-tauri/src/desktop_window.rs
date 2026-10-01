//! Keeps desktop windows inside the current monitor's usable area without zooming the UI.

use std::sync::{Arc, Mutex};

use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent};

const MIN_LOGICAL_WIDTH: f64 = 960.0;
const MIN_LOGICAL_HEIGHT: f64 = 600.0;

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
    minimum: PhysicalSize<u32>,
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
    fn needs_fit(&self, display: DisplayArea) -> bool {
        !self.applying && (self.pending || self.display != display)
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

fn placement(display: DisplayArea, current: Geometry, center: bool) -> Placement {
    // Work areas and window positions are physical pixels; client minima are
    // logical pixels. Account for any platform frame before comparing sizes.
    let frame = PhysicalSize::new(
        current.outer.width.saturating_sub(current.inner.width),
        current.outer.height.saturating_sub(current.inner.height),
    );
    let available = PhysicalSize::new(
        display.size.width.saturating_sub(frame.width).max(1),
        display.size.height.saturating_sub(frame.height).max(1),
    );
    let minimum = PhysicalSize::new(
        ((MIN_LOGICAL_WIDTH * display.scale).round() as u32).clamp(1, available.width),
        ((MIN_LOGICAL_HEIGHT * display.scale).round() as u32).clamp(1, available.height),
    );
    let inner = PhysicalSize::new(
        current.inner.width.clamp(minimum.width, available.width),
        current.inner.height.clamp(minimum.height, available.height),
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
        minimum,
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

fn apply(window: &WebviewWindow, display: DisplayArea, center: bool) -> Result<bool, String> {
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
    let target = placement(display, current, center);
    // Lower an old monitor's minimum before requesting a smaller window.
    window
        .set_min_size(Some(target.minimum))
        .map_err(|error| format!("set desktop minimum size: {error}"))?;
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

/// Called once for every newly created window, including WebView recovery.
/// The caller keeps the window hidden until its initial placement is ready.
pub(crate) fn prepare(window: &WebviewWindow) -> Result<(), String> {
    let display = display_area(window)?;
    let fitted = apply(window, display, true)?;
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
                if !state.needs_fit(display) {
                    return Ok(());
                }
                // Native setters can synchronously deliver more window events.
                // Never hold this lock while calling them or refit recursively.
                state.applying = true;
                state.pending = true;
            }
            let result = apply(&target, display, false);
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
        let fit = placement(display(1920, 1040, 1.0), window(1560, 900), true);
        assert_eq!(fit.inner, PhysicalSize::new(1560, 900));
        assert_eq!(fit.minimum, PhysicalSize::new(960, 600));
        assert_eq!(fit.position, PhysicalPosition::new(180, 70));
    }

    #[test]
    fn high_dpi_uses_work_area_instead_of_raw_resolution() {
        let fit = placement(display(1920, 1040, 1.5), window(2340, 1350), true);
        assert_eq!(fit.inner, PhysicalSize::new(1920, 1040));
        assert_eq!(fit.minimum, PhysicalSize::new(1440, 900));
        assert_eq!(fit.position, PhysicalPosition::new(0, 0));
    }

    #[test]
    fn small_work_area_reduces_the_minimum_including_frame_size() {
        let mut current = window(1560, 900);
        current.outer = PhysicalSize::new(1576, 939);
        let fit = placement(display(1280, 720, 2.0), current, true);
        assert_eq!(fit.minimum, PhysicalSize::new(1264, 681));
        assert_eq!(fit.inner, fit.minimum);
        assert_eq!(fit.position, PhysicalPosition::new(0, 0));
    }

    #[test]
    fn monitor_change_preserves_a_fitting_user_size_and_clamps_negative_coordinates() {
        let mut secondary = display(1920, 1040, 1.0);
        secondary.origin = PhysicalPosition::new(-1920, 40);
        let mut current = window(1100, 700);
        current.position = PhysicalPosition::new(-400, -200);
        let fit = placement(secondary, current, false);
        assert_eq!(fit.inner, current.inner);
        assert_eq!(fit.position, PhysicalPosition::new(-1100, 40));
    }

    #[test]
    fn window_growth_is_not_forced_when_moving_to_a_larger_work_area() {
        let mut current = window(1000, 640);
        current.position = PhysicalPosition::new(100, 80);
        let fit = placement(display(3840, 2080, 1.0), current, false);
        assert_eq!(fit.inner, current.inner);
        assert_eq!(fit.position, current.position);
    }

    #[test]
    fn ordinary_resize_does_not_request_a_fit_and_pending_dpi_change_does() {
        let display = display(1920, 1040, 1.0);
        let mut state = Adaptation {
            display,
            pending: false,
            applying: false,
            last_error: None,
        };
        assert!(!state.needs_fit(display));
        state.pending = true;
        assert!(state.needs_fit(display));
        state.applying = true;
        assert!(!state.needs_fit(display));
        state.applying = false;
        state.pending = false;
        assert!(state.needs_fit(DisplayArea {
            scale: 1.25,
            ..display
        }));
    }
}
