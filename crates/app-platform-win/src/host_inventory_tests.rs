use super::*;

fn disk(index: u32, model: &str) -> DiskRecord {
    DiskRecord {
        index,
        model: Some(model.into()),
    }
}

#[test]
fn volume_models_follow_all_extent_disk_numbers_instead_of_disk_zero() {
    assert_eq!(
        model_for_disk_numbers(
            &[7, 2, 7],
            vec![
                disk(0, "Other system disk"),
                disk(2, "SSD B"),
                disk(7, "SSD A")
            ]
        ),
        Some("SSD A / SSD B".into())
    );
    assert_eq!(
        model_for_disk_numbers(&[2, 7], vec![disk(2, "Same SSD"), disk(7, "Same SSD")]),
        Some("Same SSD".into())
    );
    assert_eq!(
        model_for_disk_numbers(&[2, 7], vec![disk(2, "Known SSD")]),
        None
    );
    assert_eq!(model_for_disk_numbers(&[2], vec![disk(2, "   ")]), None);
}

#[test]
fn volume_extents_validate_full_response_before_selecting_disks() {
    let mut output = VolumeDiskExtents {
        count: 3,
        extents: [DiskExtent::default(); MAX_DISK_EXTENTS],
    };
    output.extents[0].disk_number = 7;
    output.extents[1].disk_number = 2;
    output.extents[2].disk_number = 7;
    let full =
        std::mem::offset_of!(VolumeDiskExtents, extents) + 3 * std::mem::size_of::<DiskExtent>();
    assert_eq!(disk_numbers_from_extents(&output, full), Some(vec![2, 7]));
    assert_eq!(disk_numbers_from_extents(&output, full - 1), None);
    output.count = MAX_DISK_EXTENTS as u32 + 1;
    assert_eq!(
        disk_numbers_from_extents(&output, std::mem::size_of_val(&output)),
        None
    );
    output.count = 0;
    assert_eq!(
        disk_numbers_from_extents(&output, std::mem::size_of_val(&output)),
        None
    );
}

#[test]
fn inventory_cache_is_bounded_to_one_target_and_retries_failed_queries_after_ttl() {
    let mut cache = DiskInventoryCache::default();
    let now = Instant::now();
    assert_eq!(
        cache.sample_with(Some("volume-a"), now, |_| Some("SSD A".into())),
        "SSD A"
    );
    assert_eq!(
        cache.sample_with(Some("volume-a"), now + Duration::from_secs(60), |_| panic!(
            "must use static inventory cache"
        )),
        "SSD A"
    );
    assert_eq!(
        cache.sample_with(Some("volume-b"), now + Duration::from_secs(61), |_| None),
        ""
    );
    assert_eq!(
        cache.sample_with(
            Some("volume-b"),
            now + Duration::from_secs(120),
            |_| panic!("failure must not spawn on each poll")
        ),
        ""
    );
    assert_eq!(
        cache.sample_with(Some("volume-b"), now + Duration::from_secs(661), |_| Some(
            "SSD B".into()
        )),
        "SSD B"
    );
    assert_eq!(
        cache.sample_with(None, now + Duration::from_secs(662), |_| panic!(
            "unresolved volume must not query another disk"
        )),
        ""
    );
    assert!(cache.volume_id.is_empty());
}

#[test]
fn display_mounts_preserve_real_paths_and_never_expose_guid_namespaces() {
    assert_eq!(readable_path(r"\\?\D:\"), Some(r"D:\".into()));
    assert_eq!(
        readable_path(r"\\?\C:\mounts\games\"),
        Some(r"C:\mounts\games\".into())
    );
    assert_eq!(
        readable_path(r"\\?\UNC\host\share\"),
        Some(r"\\host\share\".into())
    );
    assert_eq!(
        readable_path(r"\\?\Volume{01234567-1234-1234-1234-0123456789ab}\"),
        None
    );
    assert_eq!(
        preferred_mount_label([r"C:\mounted-volumes\games\".into(), r"E:\".into()].into_iter()),
        r"E:\"
    );
    assert_eq!(
        preferred_mount_label([r"C:\mounted-volumes\games\".into()].into_iter()),
        r"C:\mounted-volumes\games\"
    );
}

#[test]
#[ignore = "manual read-only Windows inventory probe"]
fn live_inventory_reports_selected_volume_and_memory_modules() {
    let temp = std::env::temp_dir();
    let (id, mount) =
        crate::host_telemetry::resolve_volume_identity(&temp).expect("existing temp volume");
    let model = DiskInventoryCache::default().model_for_volume(Some(&id));
    let label = volume_display_label(&id, &mount);
    let modules = crate::read_memory_modules();
    eprintln!(
        "inventory: disk_model={model:?}; readable_mount={}; memory_modules={}; memory={:?}",
        !label.is_empty() && !label.contains("Volume{"),
        modules.len(),
        modules
            .iter()
            .map(|module| (
                &module.manufacturer,
                &module.part_number,
                module.configured_clock_mts
            ))
            .collect::<Vec<_>>()
    );
    assert!(
        !model.is_empty(),
        "this manual probe requires an accessible local disk model"
    );
    assert!(!label.is_empty());
    assert!(!label.contains("Volume{"));
    assert!(!modules.is_empty());
}
