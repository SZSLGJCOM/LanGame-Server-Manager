use super::*;

fn identity(image: &str, creation_time: u64) -> ProcessIdentity {
    ProcessIdentity {
        image_path: image.into(),
        creation_time,
    }
}
fn record(pid: u32, image: &str, creation_time: u64) -> WindowsProcessRecord {
    WindowsProcessRecord {
        process_id: pid,
        parent_process_id: 40,
        name: SHIPPING.into(),
        identity: Some(identity(image, creation_time)),
    }
}

#[test]
fn shipping_requires_the_unique_image_beneath_the_recorded_owner() {
    let root = identity("D:/fixture/MoriaServer.exe", 100);
    let image = "D:/fixture/Moria/Binaries/Win64/MoriaServer-Win64-Shipping.exe";
    let records = [record(41, image, 110)];
    assert_eq!(
        select_shipping(40, &root, &records).map(|record| record.process_id),
        Some(41)
    );
    assert!(select_shipping(40, &identity("D:/other/MoriaServer.exe", 100), &records).is_none());
    assert!(select_shipping(40, &identity("D:/fixture/OtherServer.exe", 100), &records).is_none());
    assert!(select_shipping(40, &root, &[record(41, image, 0)]).is_none());
    assert!(select_shipping(40, &root, &[record(41, image, 99)]).is_none());
    assert!(
        select_shipping(40, &root, &[record(41, image, 110), record(42, image, 120)]).is_none()
    );
}

#[test]
fn direct_shipping_owner_uses_its_exact_recorded_path() {
    let image = "D:/fixture/Moria/Binaries/Win64/MoriaServer-Win64-Shipping.exe";
    let root = identity(image, 110);
    assert!(select_shipping(41, &root, &[record(41, image, 110)]).is_some());
    assert!(select_shipping(41, &root, &[record(41, image, 111)]).is_none());
    assert!(select_shipping(41, &root, &[record(42, image, 110)]).is_none());
    assert!(
        select_shipping(
            41,
            &root,
            &[record(41, "D:/other/MoriaServer-Win64-Shipping.exe", 110)]
        )
        .is_none()
    );
}

#[test]
fn console_requires_sender_target_and_only_authorized_members() {
    assert!(member_set_valid(&[41, 90], 90, 41, &[40, 41], true));
    assert!(!member_set_valid(&[41], 90, 41, &[40, 41], true));
    assert!(!member_set_valid(&[90], 90, 41, &[40, 41], true));
    assert!(!member_set_valid(&[41, 90, 999], 90, 41, &[40, 41], true));
    assert!(!member_set_valid(&[], 90, 41, &[40, 41], true));
    // The exact pinned target may have naturally exited after the one write.
    assert!(member_set_valid(&[90], 90, 41, &[], false));
    assert!(!member_set_valid(&[90, 999], 90, 41, &[], false));
}

#[test]
fn exit_is_one_fixed_command_and_partial_delivery_never_replays() {
    let calls = std::cell::Cell::new(0);
    assert!(
        write_exit_once(41, |events| {
            calls.set(calls.get() + 1);
            let down: Vec<u16> = events
                .iter()
                .filter(|event| event.event.down == 1)
                .map(|event| event.event.character)
                .collect();
            assert_eq!(String::from_utf16(&down).unwrap(), "exit\r");
            Ok(events.len() as u32 - 1)
        })
        .is_err()
    );
    assert_eq!(calls.get(), 1);
    assert!(
        write_exit_once(41, |_| {
            calls.set(calls.get() + 1);
            Err(std::io::Error::other("lost console response"))
        })
        .is_err()
    );
    assert_eq!(calls.get(), 2);
    assert!(write_exit_once(41, |events| Ok(events.len() as u32)).is_ok());
}
