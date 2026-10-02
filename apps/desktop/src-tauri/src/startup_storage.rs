use app_storage::{StorageError, StoragePaths};
#[cfg(any(windows, test))]
use std::path::Path;

pub(crate) fn resolve() -> Result<StoragePaths, StorageError> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{IDOK, MB_ICONERROR, MB_OK, MB_OKCANCEL};

        resolve_with_recovery(
            StoragePaths::resolve_default(),
            |details| {
                let message = format!(
                    "无法自动找到可用的 LanGame 数据目录。\n\n程序不会接管无法识别归属的非空 LanGame 文件夹，也不会覆盖其中的数据。\n\n选择“确定”后，请选择另一个可写目录。程序将在该目录下创建 LanGame；若所选目录本身名为 LanGame，则使用该目录。选择“取消”退出启动。\n\n错误详情：\n{details}"
                );
                show_message(&message, MB_OKCANCEL | MB_ICONERROR) == IDOK
            },
            || crate::commands::pick_directory_path(None),
            StoragePaths::resolve_in_directory,
            |error| {
                show_message(&storage_error_message(error), MB_OK | MB_ICONERROR);
            },
        )
    }
    #[cfg(not(windows))]
    StoragePaths::resolve_default()
        .inspect_err(|error| eprintln!("{}", storage_error_message(error)))
}

fn storage_error_message(error: &StorageError) -> String {
    format!(
        "无法准备 LanGame 数据目录。\n\n请恢复或重新连接原数据位置后再启动；首次使用时，请检查本地磁盘的可用空间和写入权限。程序不会因已选位置不可用而另建数据目录。\n\n错误详情：\n{error}"
    )
}

#[cfg(windows)]
fn show_message(message: &str, flags: u32) -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;

    let message: Vec<u16> = message
        .replace('\0', "\\0")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let title: Vec<u16> = "LanGame Server Manager"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // Resolve before Builder: a modal message pump must not dispatch WebView IPC
    // before the runtime client is managed. Both buffers outlive this call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            flags,
        )
    }
}

#[cfg(any(windows, test))]
fn resolve_with_recovery<T>(
    initial: Result<T, StorageError>,
    mut request_directory: impl FnMut(&str) -> bool,
    mut pick_directory: impl FnMut() -> Result<Option<String>, String>,
    mut resolve_directory: impl FnMut(&Path) -> Result<T, StorageError>,
    mut report_error: impl FnMut(&StorageError),
) -> Result<T, StorageError> {
    let mut error = match initial {
        Ok(paths) => return Ok(paths),
        Err(error @ StorageError::NoUsableStorageLocation { .. }) => error,
        Err(error) => {
            report_error(&error);
            return Err(error);
        }
    };
    let mut details = error.to_string();
    loop {
        if !request_directory(&details) {
            return Err(error);
        }
        let directory = match pick_directory() {
            Ok(Some(directory)) => directory,
            Ok(None) => return Err(error),
            Err(reason) => {
                details = format!("无法打开目录选择器：{reason}\n\n{error}");
                continue;
            }
        };
        match resolve_directory(Path::new(&directory)) {
            Ok(paths) => return Ok(paths),
            Err(next @ StorageError::NoUsableStorageLocation { .. }) => {
                error = next;
                details = error.to_string();
            }
            Err(error) => {
                // A saved location may have appeared while the picker was open.
                // Its failure must not be treated as permission to choose anew.
                report_error(&error);
                return Err(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io;
    use std::path::PathBuf;

    fn no_location(details: &str) -> StorageError {
        StorageError::NoUsableStorageLocation {
            details: details.to_owned(),
        }
    }

    fn saved_location_error() -> StorageError {
        StorageError::ReadPath {
            path: PathBuf::from("missing-saved-data"),
            source: io::Error::from(io::ErrorKind::NotFound),
        }
    }

    #[test]
    fn resolved_location_does_not_prompt_or_pick() {
        let result = resolve_with_recovery(
            Ok("saved-location"),
            |_| panic!("successful resolution must not prompt"),
            || panic!("successful resolution must not open a picker"),
            |_| panic!("successful resolution must not select another location"),
            |_| panic!("successful resolution must not show an error"),
        );
        assert_eq!(result.unwrap(), "saved-location");
    }

    #[test]
    fn cancelling_the_prompt_keeps_the_original_error_without_a_second_dialog() {
        let original = no_location("all candidates conflict");
        let expected = original.to_string();
        let result = resolve_with_recovery::<()>(
            Err(original),
            |details| {
                assert_eq!(details, expected);
                false
            },
            || panic!("cancel must not open a picker"),
            |_| panic!("cancel must not prepare storage"),
            |_| panic!("cancel must not show another error dialog"),
        );
        assert_eq!(result.unwrap_err().to_string(), expected);
    }

    #[test]
    fn cancelling_the_picker_does_not_prepare_storage_or_show_another_error() {
        let result = resolve_with_recovery::<()>(
            Err(no_location("all candidates conflict")),
            |_| true,
            || Ok(None),
            |_| panic!("cancelled picker must not prepare storage"),
            |_| panic!("cancelled picker must not show another error dialog"),
        );
        assert!(matches!(
            result,
            Err(StorageError::NoUsableStorageLocation { .. })
        ));
    }

    #[test]
    fn a_conflict_requires_another_confirmation_before_selecting_again() {
        let events = RefCell::new(Vec::new());
        let mut choices = ["first-parent", "second-parent"].into_iter();
        let mut attempts = 0;
        let result = resolve_with_recovery(
            Err(no_location("automatic selection rejected")),
            |details| {
                events.borrow_mut().push("confirm");
                assert!(
                    details.contains("selection rejected") || details.contains("first conflict")
                );
                true
            },
            || {
                events.borrow_mut().push("pick");
                Ok(Some(
                    choices.next().expect("only two user selections").to_owned(),
                ))
            },
            |directory| {
                events.borrow_mut().push("resolve");
                attempts += 1;
                if attempts == 1 {
                    assert_eq!(directory, Path::new("first-parent"));
                    Err(no_location("first conflict"))
                } else {
                    assert_eq!(directory, Path::new("second-parent"));
                    Ok("selected-location")
                }
            },
            |_| panic!("a selectable conflict must not show the saved-location dialog"),
        );
        assert_eq!(result.unwrap(), "selected-location");
        assert_eq!(
            *events.borrow(),
            ["confirm", "pick", "resolve", "confirm", "pick", "resolve"]
        );
    }

    #[test]
    fn picker_failure_is_reported_before_the_user_decides_whether_to_retry() {
        let mut prompts = Vec::new();
        let mut picker_calls = 0;
        let result = resolve_with_recovery::<()>(
            Err(no_location("original conflict")),
            |details| {
                prompts.push(details.to_owned());
                prompts.len() == 1
            },
            || {
                picker_calls += 1;
                Err("picker unavailable".to_owned())
            },
            |_| panic!("a failed picker must not prepare storage"),
            |_| panic!("cancel must not show another error dialog"),
        );
        assert_eq!(picker_calls, 1);
        assert_eq!(prompts.len(), 2);
        assert!(prompts[1].contains("picker unavailable"));
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("original conflict")
        );
    }

    #[test]
    fn an_existing_location_error_never_offers_a_picker() {
        let original = saved_location_error();
        let expected = original.to_string();
        let mut reported = Vec::new();
        let result = resolve_with_recovery::<()>(
            Err(original),
            |_| panic!("saved-location errors must not offer another location"),
            || panic!("saved-location errors must not open a picker"),
            |_| panic!("saved-location errors must not prepare storage"),
            |error| reported.push(error.to_string()),
        );
        assert_eq!(reported.as_slice(), std::slice::from_ref(&expected));
        assert_eq!(result.unwrap_err().to_string(), expected);
    }

    #[test]
    fn a_saved_location_found_during_selection_stops_reselection() {
        let mut confirmations = 0;
        let mut reported = Vec::new();
        let result = resolve_with_recovery::<()>(
            Err(no_location("automatic selection rejected")),
            |_| {
                confirmations += 1;
                true
            },
            || Ok(Some("selected-parent".to_owned())),
            |_| Err(saved_location_error()),
            |error| reported.push(error.to_string()),
        );
        assert_eq!(confirmations, 1);
        assert_eq!(reported, [result.unwrap_err().to_string()]);
    }
}
