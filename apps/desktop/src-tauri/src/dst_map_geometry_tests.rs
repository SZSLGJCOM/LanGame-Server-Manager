use super::*;

const ROOM: &str = r#"
    local room = {inst={GUID=15},currentroomindex=0,origin={tx=30,ty=40},_lgsm_map_revision=1}
    TheWorld.components = {virtualroommanager={virtualroomsets={room,ATRIUM=room}}}
"#;

#[test]
fn virtual_room_identity_tracks_replacement_movement_removal_and_return_visits() {
    let initial = native_query_fixture(false, ROOM).unwrap();
    assert_eq!(initial.geometry_revision, "15:0:30:40:1");
    for update in [
        "r.currentroomindex=1",
        "r.origin.tx=31",
        "r.origin=nil",
        "r.inst.GUID=16",
        // The observer counts switching away and back before the next poll.
        "r._lgsm_map_revision=3",
        "TheWorld.components.virtualroommanager.virtualroomsets={}",
    ] {
        let setup = format!(
            "{ROOM} local r=TheWorld.components.virtualroommanager.virtualroomsets[1]; {update}"
        );
        let next = native_query_fixture(false, &setup).unwrap();
        assert_ne!(
            initial.geometry_revision, next.geometry_revision,
            "{update}"
        );
    }
    assert_eq!(
        native_query_fixture(true, ROOM).unwrap().geometry_revision,
        initial.geometry_revision
    );
    assert_eq!(
        native_query_fixture(false, "").unwrap().geometry_revision,
        ""
    );
}

#[test]
fn room_change_between_tile_batches_rejects_partial_geometry_and_releases_query() {
    let setup = format!(
        r#"{ROOM}
        function TheWorld.Map:GetSize() return 128,64 end
        function TheWorld:DoTaskInTime(_, fn)
            self.components.virtualroommanager.virtualroomsets[1].currentroomindex=1
            fn()
            return {{Cancel=function() end}}
        end
    "#
    );
    assert_eq!(
        native_query_fixture(true, &setup).unwrap_err(),
        "DST_MAP_GEOMETRY_CHANGED: Virtual rooms changed during collection."
    );
}

#[test]
fn virtual_room_metadata_is_bounded_before_any_query_lock_is_created() {
    let setup = format!(
        r#"{ROOM}
        local rooms=TheWorld.components.virtualroommanager.virtualroomsets
        for i=2,65 do rooms[i]=rooms[1] end
    "#
    );
    assert!(
        native_query_fixture(true, &setup)
            .unwrap_err()
            .contains("Invalid room state")
    );
    let mut capture = Capture::new("nonce", false);
    assert!(push(&mut capture, json!({"k":"begin","width":1,"height":1,"tileScale":4,
        "session":"world","geometryRevision":"x".repeat(2049),"day":1,"season":"autumn","phase":"day"})).is_err());
}

#[test]
fn rooms_without_change_observation_never_claim_a_reusable_geometry_revision() {
    let setup = format!(
        "{ROOM} TheWorld.components.virtualroommanager.virtualroomsets[1]._lgsm_map_revision=nil"
    );
    for terrain in [false, true] {
        assert_eq!(
            native_query_fixture(terrain, &setup).unwrap_err(),
            "DST map: Invalid room state; update the map observer and restart the shard."
        );
    }
}
