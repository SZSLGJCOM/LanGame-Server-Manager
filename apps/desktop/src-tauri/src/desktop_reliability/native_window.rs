//! Exercises fixed sizing against a real, isolated HWND, including recovered views.

use std::time::Duration;

use tauri::{Manager, PhysicalSize, WebviewWindow};
use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_NCHITTEST};

const TIMEOUT: Duration = Duration::from_secs(5);

async fn on_main<T: Send + 'static>(
    app: &tauri::AppHandle,
    operation: impl FnOnce(WebviewWindow) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let handle = app.clone();
    let (reply, response) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        if !reply.is_closed() {
            let result = handle
                .get_webview_window("main")
                .ok_or_else(|| "Native window fixture is missing".to_owned())
                .and_then(operation);
            let _ = reply.send(result);
        }
    })
    .map_err(|error| error.to_string())?;
    tokio::time::timeout(TIMEOUT, response)
        .await
        .map_err(|_| "Native window fixture operation timed out")?
        .map_err(|_| "Native window fixture operation lost its response")?
}

fn verify_edges(window: &WebviewWindow) -> Result<(), String> {
    if window.is_resizable().map_err(|error| error.to_string())? {
        return Err("Desktop window unexpectedly allows resizing".into());
    }
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let size = window.outer_size().map_err(|error| error.to_string())?;
    let hwnd = window.hwnd().map_err(|error| error.to_string())?;
    for x in [0, size.width / 2, size.width.saturating_sub(1)] {
        for y in [0, size.height / 2, size.height.saturating_sub(1)] {
            let screen_x = i64::from(position.x) + i64::from(x);
            let screen_y = i64::from(position.y) + i64::from(y);
            let point = ((screen_x & 0xffff) | ((screen_y & 0xffff) << 16)) as isize;
            // SAFETY: this is the fixture's live HWND on its UI thread. Hit
            // testing observes native resize regions without moving the cursor.
            let hit = unsafe { SendMessageW(hwnd.0.cast(), WM_NCHITTEST, 0, point) };
            if (10..=17).contains(&hit) {
                return Err(format!("Desktop edge exposes native resize hit code {hit}"));
            }
        }
    }
    Ok(())
}

fn verify_zoom(window: &WebviewWindow, expected: f64) -> Result<(), String> {
    let (reply, response) = std::sync::mpsc::sync_channel(1);
    window
        .with_webview(move |platform| {
            let mut actual = 0.0;
            // SAFETY: every caller is on this window's UI thread; the live
            // controller writes one f64 while this stack value is valid.
            let result = unsafe { platform.controller().ZoomFactor(&mut actual) }
                .map_err(|error| format!("Read desktop page zoom: {error}"))
                .and_then(|()| {
                    if (actual - expected).abs() <= 1e-9 {
                        Ok(())
                    } else {
                        Err(format!(
                            "Desktop page zoom is {actual}, expected {expected}"
                        ))
                    }
                });
            let _ = reply.send(result);
        })
        .map_err(|error| error.to_string())?;
    response
        .try_recv()
        .map_err(|error| format!("Native zoom check did not complete on the UI thread: {error}"))?
}

async fn wait_size(
    app: &tauri::AppHandle,
    maximized: bool,
    size: PhysicalSize<u32>,
    zoom: f64,
) -> Result<(), String> {
    tokio::time::timeout(TIMEOUT, async {
        loop {
            if on_main(app, move |window| {
                let actual = window.inner_size().map_err(|error| error.to_string())?;
                // Tao reserves one pixel for an auto-hidden taskbar. Restoring
                // windowed mode must still match the exact fixed size.
                let size_matches = if maximized {
                    actual.width <= size.width
                        && size.width - actual.width <= 1
                        && actual.height <= size.height
                        && size.height - actual.height <= 1
                } else {
                    actual == size
                };
                let ready = window.is_maximized().map_err(|error| error.to_string())? == maximized
                    && size_matches;
                if ready {
                    verify_zoom(&window, zoom)?;
                }
                Ok(ready)
            })
            .await?
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| format!("Desktop did not reach maximized={maximized}, client size={size:?}"))?
}

pub(super) async fn verify(app: &tauri::AppHandle) -> Result<(), String> {
    let (windowed, maximized, zoom) = on_main(app, |window| {
        verify_edges(&window)?;
        let windowed = window.inner_size().map_err(|error| error.to_string())?;
        let monitor = window
            .current_monitor()
            .map_err(|error| error.to_string())?
            .ok_or("Native fixture has no current monitor")?;
        let outer = window.outer_size().map_err(|error| error.to_string())?;
        let work_area = monitor.work_area().size;
        let screen = monitor.size();
        let screen_scale =
            (f64::from(screen.width) / 2560.0).min(f64::from(screen.height) / 1440.0);
        // Compute the contract independently of the production placement helper;
        // merely remembering the initial size would also accept an unadapted HWND.
        let available_width = ((u64::from(work_area.width) * 9 / 10) as u32)
            .saturating_sub(outer.width.saturating_sub(windowed.width))
            .max(1);
        let available_height = ((u64::from(work_area.height) * 9 / 10) as u32)
            .saturating_sub(outer.height.saturating_sub(windowed.height))
            .max(1);
        let window_scale = screen_scale
            .min(f64::from(available_width) / 1560.0)
            .min(f64::from(available_height) / 900.0);
        let expected = PhysicalSize::new(
            ((1560.0 * window_scale).round() as u32).clamp(1, available_width),
            ((900.0 * window_scale).round() as u32).clamp(1, available_height),
        );
        if windowed != expected {
            return Err(format!(
                "Initial desktop client size is {windowed:?}, expected {expected:?} for {screen:?}"
            ));
        }
        let zoom = screen_scale.max(0.75) / monitor.scale_factor();
        verify_zoom(&window, zoom)?;
        if u64::from(outer.width) * 10 > u64::from(work_area.width) * 9
            || u64::from(outer.height) * 10 > u64::from(work_area.height) * 9
        {
            return Err(format!(
                "Windowed desktop exceeds 90% of its work area: {outer:?}, {work_area:?}"
            ));
        }
        window.maximize().map_err(|error| error.to_string())?;
        Ok((windowed, work_area, zoom))
    })
    .await?;
    wait_size(app, true, maximized, zoom).await?;
    on_main(app, |window| {
        verify_edges(&window)?;
        window.unmaximize().map_err(|error| error.to_string())
    })
    .await?;
    wait_size(app, false, windowed, zoom).await?;
    // Use the production restore path on a real HWND without moving the user's cursor.
    on_main(app, |window| {
        window.maximize().map_err(|error| error.to_string())
    })
    .await?;
    wait_size(app, true, maximized, zoom).await?;
    let expected_position = on_main(app, |window| {
        let monitor = window
            .current_monitor()
            .map_err(|error| error.to_string())?
            .ok_or("Native fixture has no current monitor")?;
        let area = monitor.work_area();
        let cursor = tauri::PhysicalPosition::new(
            f64::from(area.position.x) + f64::from(area.size.width) / 2.0,
            f64::from(area.position.y) + 160.0,
        );
        crate::desktop_window::restore_for_drag(
            &window,
            crate::desktop_window::DragAnchor {
                x_ratio: 0.5,
                y: 24.0,
            },
            cursor,
        )
        .map_err(|error| error.to_string())?;
        let outer = window.outer_size().map_err(|error| error.to_string())?;
        Ok(tauri::PhysicalPosition::new(
            (cursor.x - f64::from(outer.width) / 2.0).round() as i32,
            (cursor.y - 24.0).round() as i32,
        ))
    })
    .await?;
    wait_size(app, false, windowed, zoom).await?;
    on_main(app, move |window| {
        if window.outer_position().map_err(|error| error.to_string())? != expected_position {
            return Err("Drag restoration did not preserve the pointer anchor".into());
        }
        verify_edges(&window)
    })
    .await?;
    // Exercise the production native snap operation without moving the user's
    // cursor. Gesture completion/cancellation is covered by the hook's tests.
    on_main(app, |window| {
        verify_edges(&window)?;
        let monitor = window
            .current_monitor()
            .map_err(|error| error.to_string())?
            .ok_or("Native fixture has no current monitor")?;
        let position = monitor.position();
        let screen = monitor.size();
        let hwnd = window.hwnd().map_err(|error| error.to_string())?.0.cast();
        let away_from_top = windows_sys::Win32::Foundation::POINT {
            x: position.x + (screen.width / 2) as i32,
            y: position.y + 100,
        };
        if crate::desktop_window::maximize_at_top_edge(hwnd, away_from_top)
            .map_err(|error| error.to_string())?
        {
            return Err("An ordinary drag end unexpectedly requested maximization".into());
        }
        let top = windows_sys::Win32::Foundation::POINT {
            x: away_from_top.x,
            y: position.y,
        };
        if !crate::desktop_window::maximize_at_top_edge(hwnd, top)
            .map_err(|error| error.to_string())?
        {
            return Err("A top-edge drag end did not request maximization".into());
        }
        Ok(())
    })
    .await?;
    wait_size(app, true, maximized, zoom).await?;
    on_main(app, |window| {
        verify_edges(&window)?;
        window.unmaximize().map_err(|error| error.to_string())
    })
    .await?;
    wait_size(app, false, windowed, zoom).await?;
    on_main(app, move |window| {
        verify_edges(&window)?;
        // A programmatic or OS resize must also be corrected to the fixed size.
        window
            .set_size(PhysicalSize::new(windowed.width / 2, windowed.height))
            .map_err(|error| error.to_string())
    })
    .await?;
    wait_size(app, false, windowed, zoom).await
}
