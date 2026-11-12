//! Read the native controller rectangle in main-client physical coordinates.
use super::*;
use windows_sys::Win32::{
    Foundation::{GetLastError, HWND, POINT, RECT, SetLastError},
    Graphics::Gdi::MapWindowPoints,
    UI::{
        Input::KeyboardAndMouse::GetFocus,
        WindowsAndMessaging::{GetClientRect, IsChild},
    },
};

#[derive(Debug, serde::Serialize)]
pub(super) struct BackgroundGeometry {
    controller_in_main_client: [i32; 4],
    main_client: [i32; 4],
    controller_rendering_enabled: bool,
    has_native_focus: bool,
}

impl BackgroundGeometry {
    fn validate(&self) -> Result<(), String> {
        let [left, top, right, bottom] = self.controller_in_main_client;
        let [client_left, client_top, client_right, client_bottom] = self.main_client;
        if left >= right
            || top >= bottom
            || client_left >= client_right
            || client_top >= client_bottom
        {
            return Err(format!(
                "Background geometry was empty or inverted: {self:?}"
            ));
        }
        if left < client_right && right > client_left && top < client_bottom && bottom > client_top
        {
            return Err(format!(
                "Background website intersected the main client area: {self:?}"
            ));
        }
        if !self.controller_rendering_enabled || self.has_native_focus {
            return Err(format!(
                "Background website stopped rendering or took focus: {self:?}"
            ));
        }
        Ok(())
    }
}

pub(super) async fn background_geometry(view: &Webview) -> Result<BackgroundGeometry, String> {
    let main = view.window();
    if main.label() != "main" || view.app_handle().windows().len() != 1 {
        return Err("Background website created another top-level window".into());
    }
    let main_hwnd = main.hwnd().map_err(|error| error.to_string())?.0 as usize;
    let (reply, response) = tokio::sync::oneshot::channel();
    view.with_webview(move |platform| {
        let result = (|| -> Result<BackgroundGeometry, String> {
            // SAFETY: these controller and HWND queries run on WebView2's STA.
            unsafe {
                let controller = platform.controller();
                let mut local = Default::default();
                let mut parent = Default::default();
                let mut visible = BOOL::default();
                controller
                    .Bounds(&mut local)
                    .map_err(|error| error.to_string())?;
                controller
                    .ParentWindow(&mut parent)
                    .map_err(|error| error.to_string())?;
                controller
                    .IsVisible(&mut visible)
                    .map_err(|error| error.to_string())?;
                let main_hwnd = main_hwnd as HWND;
                let parent_hwnd = parent.0 as HWND;
                if IsChild(main_hwnd, parent_hwnd) == 0 {
                    return Err("Background controller lost its main-window child HWND".into());
                }
                let mut client = RECT::default();
                if GetClientRect(main_hwnd, &mut client) == 0 {
                    return Err("Could not query the main client rectangle".into());
                }
                // Wry puts the controller inside a separate child HWND. Its Bounds
                // are local to that child, so comparing them directly with main's
                // client rectangle would mistake the normal (0, 0) for an exposure.
                let mut corners = [
                    POINT {
                        x: local.left,
                        y: local.top,
                    },
                    POINT {
                        x: local.right,
                        y: local.bottom,
                    },
                ];
                SetLastError(0);
                if MapWindowPoints(parent_hwnd, main_hwnd, corners.as_mut_ptr(), 2) == 0
                    && GetLastError() != 0
                {
                    return Err(
                        "Could not map the controller rectangle into the main client".into(),
                    );
                }
                let focused = GetFocus();
                let observed = BackgroundGeometry {
                    controller_in_main_client: [
                        corners[0].x,
                        corners[0].y,
                        corners[1].x,
                        corners[1].y,
                    ],
                    main_client: [client.left, client.top, client.right, client.bottom],
                    controller_rendering_enabled: visible.as_bool(),
                    has_native_focus: focused == parent_hwnd || IsChild(parent_hwnd, focused) != 0,
                };
                observed.validate()?;
                Ok(observed)
            }
        })();
        let _ = reply.send(result);
    })
    .map_err(|error| error.to_string())?;
    tokio::time::timeout(Duration::from_secs(3), response)
        .await
        .map_err(|_| "Background geometry query timed out".to_owned())?
        .map_err(|_| "Background geometry query was dropped".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(bounds: [i32; 4]) -> BackgroundGeometry {
        BackgroundGeometry {
            controller_in_main_client: bounds,
            main_client: [0, 0, 1100, 800],
            controller_rendering_enabled: true,
            has_native_focus: false,
        }
    }

    #[test]
    fn background_geometry_rejects_one_pixel_overlap_and_hidden_controllers() {
        for bounds in [
            [-1100, -800, -50, -40],
            [-1050, 0, 0, 760],
            [0, -760, 1050, 0],
        ] {
            assert!(observation(bounds).validate().is_ok());
        }
        for bounds in [
            [-1049, 0, 1, 760],
            [0, -759, 1050, 1],
            [0, 0, 1050, 760],
            [-1, -1, 1101, 801],
            [-50, -40, -50, -40],
        ] {
            assert!(observation(bounds).validate().is_err());
        }
        let mut hidden = observation([-1100, -800, -50, -40]);
        hidden.controller_rendering_enabled = false;
        assert!(hidden.validate().is_err());
        hidden.controller_rendering_enabled = true;
        hidden.has_native_focus = true;
        assert!(hidden.validate().is_err());
        hidden.has_native_focus = false;
        hidden.main_client = [0, 0, 0, 0];
        assert!(hidden.validate().is_err());
    }
}
