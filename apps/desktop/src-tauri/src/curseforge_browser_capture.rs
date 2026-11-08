//! Optional visual evidence for the isolated native acceptance host.
//! Captures the real page only; it never supplies a fabricated verification page.
use super::*;
use base64::Engine;
use webview2_com::CallDevToolsProtocolMethodCompletedHandler;

pub(super) async fn run(
    app: &tauri::AppHandle,
    url: &str,
    root: &std::path::Path,
) -> Result<serde_json::Value, String> {
    let id = open_curseforge_browser_host(app.clone(), None, Channel::new(|_| Ok(())))?;
    let session = host::Resolving::begin(app, &id)?;
    let view = session.attach(url)?;
    let started = Instant::now();
    let (page_state, project) = loop {
        if started.elapsed() > Duration::from_secs(45) {
            return Err("The official page did not become ready for capture".into());
        }
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        view.0
            .eval_with_callback(include_str!("curseforge_page_extract.js"), move |value| {
                let _ = sender.try_send(value);
            })
            .map_err(|error| error.to_string())?;
        if let Ok(Some(value)) = tokio::time::timeout(Duration::from_secs(3), receiver.recv()).await
            && let Ok(page) = serde_json::from_str::<Page>(&value)
            && page.url != "about:blank"
        {
            let state = page.state.clone();
            let project = read_project(page, url)?;
            if project.is_some()
                || (state == "challenge" && started.elapsed() >= VERIFICATION_GRACE)
            {
                break (state, project);
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    // Only this opt-in test asks to display a normal page as a layout preview.
    // The enclosing native window remains hidden throughout.
    session.set_progress(host::Progress::VerificationRequired)?;
    let bounds =
        serde_json::from_value(json!({ "x": 0, "y": 0, "width": 1446, "height": 746 })).unwrap();
    update_curseforge_browser_host(app.clone(), id, Some(bounds)).await?;
    protocol(
        &view.0,
        "Emulation.setDeviceMetricsOverride",
        json!({
            "width": 1446, "height": 746, "deviceScaleFactor": 1, "mobile": false,
        }),
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let response = protocol(
        &view.0,
        "Page.captureScreenshot",
        json!({ "format": "png", "captureBeyondViewport": false }),
    )
    .await?;
    let image = base64::engine::general_purpose::STANDARD
        .decode(
            response["data"]
                .as_str()
                .ok_or("Native page capture returned no PNG")?,
        )
        .map_err(|error| error.to_string())?;
    std::fs::write(root.join("page.png"), image).map_err(|error| error.to_string())?;
    Ok(
        json!({ "status": "captured", "url": url, "page_state": page_state,
        "project_id": project.map(|project| project.id), "verification_trigger": "simulated",
        "native_page_capture": true, "parent_visible": app.get_window("main").unwrap().is_visible().map_err(|error| error.to_string())? }),
    )
}

async fn protocol(
    view: &tauri::Webview,
    method: &'static str,
    parameters: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    view.with_webview(move |platform| {
        let callback_sender = sender.clone();
        let callback =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, value| {
                let _ = callback_sender
                    .try_send(status.map(|()| value).map_err(|error| error.to_string()));
                Ok(())
            }));
        // SAFETY: controller access and callback dispatch stay on the WebView2
        // STA. This uses its local COM API and exposes no debugging listener.
        let method: Vec<_> = method.encode_utf16().chain(Some(0)).collect();
        let parameters: Vec<_> = parameters
            .to_string()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let result = unsafe {
            platform.controller().CoreWebView2().and_then(|core| {
                core.CallDevToolsProtocolMethod(
                    windows_core::PCWSTR(method.as_ptr()),
                    windows_core::PCWSTR(parameters.as_ptr()),
                    &callback,
                )
            })
        };
        if let Err(error) = result {
            let _ = sender.try_send(Err(error.to_string()));
        }
    })
    .map_err(|error| error.to_string())?;
    let response = tokio::time::timeout(Duration::from_secs(10), receiver.recv())
        .await
        .map_err(|_| "Native page capture timed out")?
        .ok_or("Native page capture lost its response")??;
    if response.len() > 16 * 1024 * 1024 {
        return Err("Native page capture exceeded its size bound".into());
    }
    serde_json::from_str(&response).map_err(|error| error.to_string())
}
