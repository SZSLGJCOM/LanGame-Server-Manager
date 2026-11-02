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

async fn wait_size(
    app: &tauri::AppHandle,
    maximized: bool,
    size: PhysicalSize<u32>,
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
                Ok(
                    window.is_maximized().map_err(|error| error.to_string())? == maximized
                        && size_matches,
                )
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
    let (windowed, maximized) = on_main(app, |window| {
        verify_edges(&window)?;
        let windowed = window.inner_size().map_err(|error| error.to_string())?;
        let monitor = window
            .current_monitor()
            .map_err(|error| error.to_string())?
            .ok_or("Native fixture has no current monitor")?;
        window.maximize().map_err(|error| error.to_string())?;
        Ok((windowed, monitor.work_area().size))
    })
    .await?;
    wait_size(app, true, maximized).await?;
    on_main(app, |window| {
        verify_edges(&window)?;
        window.unmaximize().map_err(|error| error.to_string())
    })
    .await?;
    wait_size(app, false, windowed).await?;
    on_main(app, move |window| {
        verify_edges(&window)?;
        // A programmatic or OS resize must also be corrected to the fixed size.
        window
            .set_size(PhysicalSize::new(windowed.width / 2, windowed.height))
            .map_err(|error| error.to_string())
    })
    .await?;
    wait_size(app, false, windowed).await
}
