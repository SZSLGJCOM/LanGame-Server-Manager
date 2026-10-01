use super::*;
use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
use windows_sys::Win32::Security::{ACCESS_ALLOWED_ACE, DACL_SECURITY_INFORMATION, GetAce};

async fn wait_for_closed_name(endpoint: &Endpoint) -> NamedPipeServer {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match create_pipe(endpoint, true) {
            Ok(pipe) => return pipe,
            Err(error)
                if matches!(error.raw_os_error(), Some(5 | 231))
                    && tokio::time::Instant::now() < deadline =>
            {
                // Drop initiates IO cancellation; permit the real reactor to
                // release its outstanding handle before reclaiming the name.
                tokio::task::yield_now().await;
            }
            Err(error) => panic!("The closed pipe name did not become reusable: {error}"),
        }
    }
}

#[tokio::test]
async fn pipe_grants_only_the_current_user_and_cannot_be_squatted_while_owned() {
    let endpoint = Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let server = create_pipe(&endpoint, true).unwrap();
    assert!(create_pipe(&endpoint, true).is_err());

    // GetSecurityInfo owns the descriptor; all borrowed ACL/ACE/SID pointers
    // remain inside that allocation until LocalFree below.
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        let mut dacl = std::ptr::null_mut();
        let code = GetSecurityInfo(
            server.as_raw_handle().cast(),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        );
        assert_eq!(code, 0);
        assert!(
            !dacl.is_null(),
            "A null DACL would allow every local account"
        );
        assert_eq!(
            (*dacl).AceCount,
            1,
            "Only the current user may access the control channel"
        );
        let mut entry = std::ptr::null_mut();
        assert_ne!(GetAce(dacl, 0, &mut entry), 0);
        let allowed = &*entry.cast::<ACCESS_ALLOWED_ACE>();
        assert_eq!(
            allowed.Header.AceType, 0,
            "The single ACE must grant the selected user's access"
        );
        let mut sid_text = std::ptr::null_mut();
        assert_ne!(
            ConvertSidToStringSidW(
                (&allowed.SidStart as *const u32).cast_mut().cast(),
                &mut sid_text
            ),
            0
        );
        let mut length = 0;
        while *sid_text.add(length) != 0 {
            length += 1;
        }
        let actual = String::from_utf16_lossy(std::slice::from_raw_parts(sid_text, length));
        LocalFree(sid_text.cast());
        LocalFree(descriptor);
        assert_eq!(actual, user_sid().unwrap());
    }

    let client = connect_verified(&endpoint).unwrap();
    server.connect().await.unwrap();
    drop(client);
    drop(server);
    drop(wait_for_closed_name(&endpoint).await);
}

#[test]
fn fixture_namespace_cannot_escape_its_named_pipe_prefix() {
    for nonce in ["", "short", "../some-other-pipe", "a\\b", "a/b"] {
        assert!(Endpoint::isolated(nonce).is_err());
    }
}

#[tokio::test]
async fn saturated_ordinary_connections_leave_the_dedicated_control_channel_available() {
    let endpoint = Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let mut listener = create_pipe(&endpoint, true).unwrap();
    let mut connections = Vec::new();
    for index in 0..MAX_CONNECTIONS {
        let client = connect_verified(&endpoint).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), listener.connect())
            .await
            .expect("Local connection should be accepted")
            .unwrap();
        let next = create_pipe(&endpoint, false).unwrap_or_else(|error| {
            panic!("Connection {index} consumed the reserved listener: {error}")
        });
        connections.push((std::mem::replace(&mut listener, next), client));
    }
    assert_eq!(
        connections.len(),
        34,
        "32 work and two auxiliary connections are promised"
    );
    assert_eq!(MAX_PIPE_INSTANCES, 35);
    assert!(
        create_pipe(&endpoint, false).is_err(),
        "Pipe handles must remain bounded at admitted connections plus one listener"
    );
    assert!(
        create_pipe(&endpoint, true).is_err(),
        "Saturation must not release the secured name"
    );
    let control_endpoint = endpoint.control();
    let control = create_pipe(&control_endpoint, true).unwrap();
    assert!(create_pipe(&control_endpoint, true).is_err());
    let control_client = connect_verified(&control_endpoint).unwrap();
    tokio::time::timeout(Duration::from_secs(2), control.connect())
        .await
        .expect("Ordinary connection saturation must not delay control admission")
        .unwrap();
    let identity = service_identity(&connections[0].1).unwrap();
    verify_service_identity(&control_client, &identity).unwrap();
    assert_eq!(identity.pid, std::process::id());
    drop(control_client);
    drop(control);
    drop(wait_for_closed_name(&control_endpoint).await);
    connections.pop();
    let replacement = create_replacement_pipe(&endpoint)
        .await
        .expect("One closed connection must return one pipe slot after IOCP drains");
    drop(replacement);
    drop(listener);
    drop(connections);
    drop(wait_for_closed_name(&endpoint).await);
}
