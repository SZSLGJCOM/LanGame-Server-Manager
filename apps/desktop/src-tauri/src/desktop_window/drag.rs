//! Restore fixed-size windows under the pointer before handing movement to the OS.

use serde::Deserialize;
use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DragAnchor {
    pub(crate) x_ratio: f64,
    /// Physical pixels from the client top, including the WebView's page zoom.
    pub(crate) y: f64,
}

fn drag_position(
    cursor: PhysicalPosition<f64>,
    size: PhysicalSize<u32>,
    anchor: DragAnchor,
) -> PhysicalPosition<i32> {
    PhysicalPosition::new(
        (cursor.x - f64::from(size.width) * anchor.x_ratio).round() as i32,
        (cursor.y - anchor.y).round() as i32,
    )
}

#[cfg(windows)]
pub(super) fn primary_button_held() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
    // SAFETY: these calls only read the primary button mapping and state.
    unsafe {
        let button = if GetSystemMetrics(SM_SWAPBUTTON) != 0 {
            VK_RBUTTON
        } else {
            VK_LBUTTON
        };
        GetAsyncKeyState(i32::from(button)) < 0
    }
}

/// Run on the UI thread: restore, read the fitted size, then preserve the grab point.
pub(crate) fn restore_for_drag(
    window: &WebviewWindow,
    anchor: DragAnchor,
    cursor: PhysicalPosition<f64>,
) -> tauri::Result<()> {
    if window.is_maximized()? {
        window.unmaximize()?;
        let size = window.outer_size()?;
        window.set_position(drag_position(cursor, size, anchor))?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn start_desktop_window_drag(
    window: WebviewWindow,
    anchor: DragAnchor,
) -> Result<(), String> {
    if window.label() != "main"
        || !anchor.x_ratio.is_finite()
        || !(0.0..=1.0).contains(&anchor.x_ratio)
        || !anchor.y.is_finite()
        || anchor.y < 0.0
        || anchor.y
            > f64::from(
                window
                    .inner_size()
                    .map_err(|error| error.to_string())?
                    .height,
            )
    {
        return Err("Invalid desktop drag anchor".into());
    }
    let (reply, response) = tokio::sync::oneshot::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let result = (|| -> tauri::Result<()> {
                // A quick release can reach Windows before the WebView's IPC request.
                #[cfg(windows)]
                if !primary_button_held() {
                    return Ok(());
                }
                let cursor = target.cursor_position()?;
                restore_for_drag(&target, anchor, cursor)?;
                target.start_dragging()
            })();
            let _ = reply.send(result.map_err(|error| error.to_string()));
        })
        .map_err(|error| error.to_string())?;
    response.await.map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_window_keeps_left_middle_and_right_grab_points_under_cursor() {
        for (x_ratio, expected_x) in [(0.1, 1124), (0.5, 500), (0.9, -124)] {
            assert_eq!(
                drag_position(
                    PhysicalPosition::new(1280.0, 100.0),
                    PhysicalSize::new(1560, 900),
                    DragAnchor { x_ratio, y: 24.0 }
                ),
                PhysicalPosition::new(expected_x, 76),
            );
        }
    }

    #[test]
    fn physical_anchor_supports_scaled_and_negative_coordinate_monitors() {
        assert_eq!(
            drag_position(
                PhysicalPosition::new(-900.0, -100.0),
                PhysicalSize::new(1170, 675),
                DragAnchor {
                    x_ratio: 0.75,
                    y: 36.0
                }
            ),
            PhysicalPosition::new(-1778, -136),
        );
    }
}
