use super::*;

pub(super) async fn verify(
    state: &DesktopState,
    id: &str,
    process: &StartedProcess,
    run_id: i64,
    scenario: &str,
    mod_ids: &[String],
    events: &[&str],
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut features = Vec::new();
    let (prelude, expected) = if scenario == "events" {
        let ids = events
            .iter()
            .map(|id| format!("'{id}'"))
            .collect::<Vec<_>>()
            .join(",");
        (
            format!(
                "local r={{}};for _,id in ipairs({{{ids}}}) do r[#r+1]=id..'='..tostring(IsSpecialEventActive(id)) end;"
            ),
            events
                .iter()
                .map(|id| format!("{id}=true"))
                .collect::<Vec<_>>()
                .join(";"),
        )
    } else {
        let ids = mod_ids
            .iter()
            .map(|id| format!("'{id}'"))
            .collect::<Vec<_>>()
            .join(",");
        (
            format!(
                "local r={{}};for _,id in ipairs({{{ids}}}) do local name='workshop-'..id;local m=ModManager:GetMod(name);local ok=m~=nil and m.modinfo~=nil and not m.modinfo.failed and KnownModIndex:IsModEnabledAny(name) and #ModManager.failedmods==0;r[#r+1]=id..'='..tostring(ok) end;"
            ),
            mod_ids
                .iter()
                .map(|id| format!("{id}=true"))
                .collect::<Vec<_>>()
                .join(";"),
        )
    };
    if scenario != "vanilla" {
        let actual =
            native_probe(state, id, process, run_id, &prelude, "table.concat(r,';')").await?;
        assert_eq!(actual, expected, "{} {scenario}", process.process_key);
        features.push(json!({"shard": process.process_key, "result": actual}));
    }
    if scenario == "tropical" {
        // These default Tropical Experience tasks become topology IDs only
        // when their regions actually enter the native generated map.
        let markers = if process.process_key == "master" {
            ["TROPICAL6", "Mpainted_sands", "FrostIsland_Beach"]
        } else {
            ["HamMudWorld", "Frostcavetask", "vulcaonacaverna"]
        };
        let ids = markers.map(|marker| format!("'{marker}'")).join(",");
        let topology = native_probe(
            state, id, process, run_id,
            &format!(
                "local r={{}};for _,m in ipairs({{{ids}}}) do local n=0;for _,v in ipairs(TheWorld.topology.ids) do if string.find(v,m,1,true) then n=n+1 end end;r[#r+1]=m..'='..n end;"
            ),
            "table.concat(r,';')",
        ).await?;
        let regions = topology.split(';').collect::<Vec<_>>();
        assert_eq!(regions.len(), markers.len(), "{topology}");
        for (region, marker) in regions.iter().zip(markers) {
            let (name, count) = region.split_once('=').expect("topology count");
            assert_eq!(name, marker);
            assert!(count.parse::<usize>()? > 0, "{topology}");
        }
        features.push(json!({"shard": process.process_key, "generated_regions": topology}));
    }
    Ok(features)
}
