//! Real WebView2 DOM drops and the production path bridge, with no installation.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tauri::Manager;
use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
use windows_core::PCWSTR;

const PREPARE_PAGE: &str = r#"
(async () => {
  const { droppedFilePaths } = await import('/src/desktop-file-drop.ts');
  const { referenceCandidatesFromText } = await import('/src/views/servers/mod-reference-parser.ts');
  const zone = document.createElement('div');
  zone.id = 'native-drop-probe';
  Object.assign(zone.style, {position:'fixed',left:'10px',top:'10px',width:'300px',height:'80px',zIndex:'2147483647'});
  const state = {over:false,protected:true,trusted:true,types:[],replies:0,pending:null,lastReplyError:null};
  const bridge = chrome.webview;
  const response = event => { if (event.data?.type === 'lgsm:dropped-files') {
    state.replies++; state.lastReplyError = typeof event.data.error === 'string' ? event.data.error.slice(0,64) : null;
  }};
  bridge.addEventListener('message', response);
  zone.addEventListener('dragover', event => {
    event.preventDefault(); event.dataTransfer.dropEffect = 'copy'; state.over = true;
    state.trusted &&= event.isTrusted;
    state.protected &&= event.dataTransfer.getData('text/plain') === '' && event.dataTransfer.getData('text/uri-list') === '';
  });
  zone.addEventListener('drop', event => {
    event.preventDefault();
    state.trusted &&= event.isTrusted;
    const transfer = event.dataTransfer;
    state.types = Array.from(transfer.types);
    if (state.types.includes('Files')) {
      state.pending = droppedFilePaths(transfer, new AbortController().signal)
        .then(paths => ({paths,references:[],types:state.types,over:state.over,protected:state.protected,trusted:state.trusted,replies:state.replies}));
    } else {
      const references = referenceCandidatesFromText(transfer.getData('text/plain') + '\n' + transfer.getData('text/uri-list'));
      state.pending = Promise.resolve({paths:[],references,types:state.types,over:state.over,protected:state.protected,trusted:state.trusted,replies:state.replies});
    }
  });
  state.cleanup = () => {
    zone.remove(); bridge.removeEventListener('message', response);
    delete window.__lgsmDropProbe;
  };
  window.__lgsmDropProbe = state;
  document.body.appendChild(zone);
  return true;
})()
"#;

pub(super) fn prepare(root: &Path) -> Result<(), String> {
    let drop = root.join("drop-probe");
    std::fs::create_dir(&drop).map_err(|_| "Create isolated drop fixture directory failed")?;
    std::fs::create_dir(drop.join("folder"))
        .map_err(|_| "Create isolated dropped folder failed")?;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(drop.join("generated.zip"))
        .map_err(|_| "Create isolated dropped file failed")?;
    // Path-resolution acceptance does not open or install this synthetic archive.
    file.write_all(b"synthetic dropped-file path fixture\n")
        .map_err(|_| "Write isolated dropped file failed")?;
    Ok(())
}

type Reply = Arc<Mutex<Option<tokio::sync::oneshot::Sender<Result<Value, String>>>>>;

fn complete(reply: &Reply, value: Result<Value, String>) {
    if let Some(sender) = reply
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
    {
        let _ = sender.send(value);
    }
}

async fn cdp(
    window: &tauri::WebviewWindow,
    method: &'static str,
    parameters: Value,
) -> Result<Value, String> {
    let (sender, response) = tokio::sync::oneshot::channel();
    let reply: Reply = Arc::new(Mutex::new(Some(sender)));
    window
        .with_webview(move |platform| {
            if reply
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_ref()
                .is_none_or(|sender| sender.is_closed())
            {
                return;
            }
            let setup = (|| -> windows_core::Result<()> {
                // SAFETY: this closure and completion run on the WebView2 STA.
                let core = unsafe { platform.controller().CoreWebView2() }?;
                let callback_reply = Arc::clone(&reply);
                let callback = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
                    move |result, message| {
                        let result = result
                            .map_err(|_| "Native drop DevTools request failed".to_owned())
                            .and_then(|()| {
                                if message.len() > 64 * 1024 {
                                    return Err(
                                        "Native drop DevTools reply exceeds its bound".into()
                                    );
                                }
                                serde_json::from_str(&message)
                                    .map_err(|_| "Invalid native drop DevTools response".into())
                            });
                        complete(&callback_reply, result);
                        Ok(())
                    },
                ));
                let method = method.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
                let parameters = parameters
                    .to_string()
                    .encode_utf16()
                    .chain(Some(0))
                    .collect::<Vec<_>>();
                unsafe {
                    core.CallDevToolsProtocolMethod(
                        PCWSTR(method.as_ptr()),
                        PCWSTR(parameters.as_ptr()),
                        &callback,
                    )
                }
            })();
            if setup.is_err() {
                complete(
                    &reply,
                    Err("Dispatch native drop DevTools request failed".into()),
                );
            }
        })
        .map_err(|_| "Dispatch native drop probe to the UI thread failed")?;
    tokio::time::timeout(Duration::from_secs(6), response)
        .await
        .map_err(|_| "Native drop DevTools request timed out")?
        .map_err(|_| "Native drop DevTools response was lost")?
}

async fn evaluate(
    window: &tauri::WebviewWindow,
    phase: &str,
    expression: &str,
) -> Result<Value, String> {
    let reply = cdp(window, "Runtime.evaluate", json!({"expression":expression,"awaitPromise":true,"returnByValue":true,"userGesture":true}))
        .await.map_err(|error| format!("Native drop [{phase}]: {error}"))?;
    if let Some(details) = reply.get("exceptionDetails") {
        // This feature-only page uses generated fixture files and constant test
        // inputs. Keep its actual bounded exception rather than losing the cause.
        let description = details
            .get("exception")
            .and_then(|exception| exception.get("description"))
            .and_then(Value::as_str);
        let text = details.get("text").and_then(Value::as_str);
        let diagnostic = [text, description]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("; ");
        let diagnostic = if diagnostic.is_empty() {
            "No exception text was returned".to_owned()
        } else {
            diagnostic.chars().take(1024).collect::<String>()
        };
        return Err(format!(
            "Native drop [{phase}] page exception: {diagnostic}"
        ));
    }
    reply
        .get("result")
        .and_then(|result| result.get("value"))
        .cloned()
        .ok_or_else(|| format!("Native drop [{phase}] page probe did not return a value"))
}

pub(super) async fn verify(app: &tauri::AppHandle, root: &Path) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("Native drop fixture main view is absent")?;
    let result = tokio::time::timeout(Duration::from_secs(10), verify_window(&window, root))
        .await
        .map_err(|_| "Native drop acceptance exceeded ten seconds".to_owned())
        .and_then(|result| result);
    if let Err(error) = result {
        let page = evaluate(&window, "failure/diagnostics", r#"(() => {
            const s=window.__lgsmDropProbe;
            return s ? {prepared:true,native_replies:s.replies,last_reply_error:s.lastReplyError} : {prepared:false};
        })()"#).await.unwrap_or_else(|_| json!({"page_diagnostics_unavailable":true}));
        let native = crate::webview_drop::diagnostics(&window)
            .await
            .unwrap_or_else(|_| json!({"native_diagnostics_unavailable":true}));
        let cleanup = evaluate(
            &window,
            "failure/cleanup",
            "window.__lgsmDropProbe?.cleanup();true",
        )
        .await;
        return Err(format!(
            "{error}; fixture_drop_diagnostics={page}; native_drop_diagnostics={native}; probe_cleanup={}",
            cleanup.is_ok()
        ));
    }
    Ok(())
}

async fn verify_window(window: &tauri::WebviewWindow, root: &Path) -> Result<(), String> {
    let prepared = evaluate(window, "prepare/imports", PREPARE_PAGE).await?;
    if prepared != true {
        return Err("Native drop page probe was not installed".into());
    }
    let file = root.join("drop-probe/generated.zip");
    let folder = root.join("drop-probe/folder");
    for (mime, data, files) in [
        ("Files", "", vec![file.clone(), folder.clone()]),
        ("text/plain", "1346144", Vec::new()),
        (
            "text/uri-list",
            "https://www.curseforge.com/projects/1346144",
            Vec::new(),
        ),
    ] {
        evaluate(
            window,
            &format!("{mime}/reset"),
            "Object.assign(window.__lgsmDropProbe,{over:false,protected:true,trusted:true,pending:null});true",
        )
        .await?;
        let items = if files.is_empty() {
            json!([{"mimeType":mime,"data":data}])
        } else {
            json!([])
        };
        for kind in ["dragEnter", "dragOver", "drop"] {
            cdp(window, "Input.dispatchDragEvent", json!({"type":kind,"x":50,"y":50,"data":{"items":items,"files":files,"dragOperationsMask":1}})).await?;
        }
        let result = evaluate(
            window,
            &format!("{mime}/drop-result"),
            "window.__lgsmDropProbe.pending",
        )
        .await?;
        if result["over"] != true
            || result["protected"] != true
            || result["trusted"] != true
            || !result["types"]
                .as_array()
                .is_some_and(|types| types.iter().any(|value| value == mime))
        {
            return Err(
                "Real DOM dragover did not expose protected data with the expected type".into(),
            );
        }
        if mime == "Files" {
            let paths = result["paths"]
                .as_array()
                .ok_or("Native dropped-file reply paths are missing")?;
            if paths.len() != 2 || result["replies"] != 1 {
                return Err("Native dropped-file bridge did not resolve one file and one directory exactly once".into());
            }
            for expected in [&file, &folder] {
                if !paths.iter().filter_map(Value::as_str).any(|actual| {
                    dunce::simplified(&PathBuf::from(actual))
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&expected.to_string_lossy())
                }) {
                    return Err(
                        "Native dropped-file bridge returned unexpected isolated paths".into(),
                    );
                }
            }
        } else if !result["references"]
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value == data))
        {
            return Err("Real DOM text drop did not reach the production reference parser".into());
        }
    }
    evaluate(window, "cleanup", "window.__lgsmDropProbe.cleanup();true").await?;
    Ok(())
}
