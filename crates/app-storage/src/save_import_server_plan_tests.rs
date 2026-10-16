use super::*;
use crate::backups::save_import::tests::Fixture;

fn grouped() -> (Fixture, ServerPlan) {
    let mut fixture = Fixture::new();
    fixture.context.record.summary.module_id = "sevendaystodie".into();
    let generated = instance_root(&fixture.context.record).join("GeneratedWorlds");
    fs::create_dir(&generated).unwrap();
    fs::write(generated.join("old-map"), b"original generated map").unwrap();
    let plan = ServerPlan::new(
        vec![
            ServerScope::new(
                fixture.context.target.clone(),
                vec![ServerMapping {
                    source: "".into(),
                    target: "selected".into(),
                    replace: true,
                }],
            ),
            ServerScope::new(
                generated,
                vec![ServerMapping {
                    source: "region".into(),
                    target: "selected-map".into(),
                    replace: true,
                }],
            ),
        ],
        vec![],
    );
    (fixture, plan)
}

#[test]
fn save_import_server_plan_group_failure_restores_both_scopes_and_source() {
    let (fixture, plan) = grouped();
    let before: Vec<_> = plan
        .scopes
        .iter()
        .map(|scope| io::inventory(&scope.target, None).unwrap())
        .collect();
    let source = io::inventory(&fixture.source, None).unwrap();
    let preview =
        server_transaction::preview(&fixture.context, &fixture.source, &source, plan.clone())
            .unwrap();
    let mut count = 0;
    let result = server_transaction::import(
        &fixture.context,
        &fixture.source,
        &source,
        plan.clone(),
        &preview.fingerprint,
        &publication::Publication::uncoordinated(),
        |staging, target| {
            count += 1;
            if count == 2 {
                return Err(invalid(target, "injected second-scope publication failure"));
            }
            publish_new_directory(staging, target)
        },
    );
    assert!(result.is_err());
    assert_eq!(count, 2);
    for (scope, expected) in plan.scopes.iter().zip(before) {
        assert_eq!(io::inventory(&scope.target, None).unwrap(), expected);
    }
    assert_eq!(io::inventory(&fixture.source, None).unwrap(), source);
}

#[test]
fn save_import_server_plan_group_backup_uses_owned_standard_restore_scopes() {
    let (fixture, plan) = grouped();
    let source = io::inventory(&fixture.source, None).unwrap();
    let preview =
        server_transaction::preview(&fixture.context, &fixture.source, &source, plan.clone())
            .unwrap();
    let result = server_transaction::import(
        &fixture.context,
        &fixture.source,
        &source,
        plan.clone(),
        &preview.fingerprint,
        &publication::Publication::uncoordinated(),
        publish_new_directory,
    )
    .unwrap();
    assert_eq!(result.safeguard_backup_paths.len(), 2);
    for (path, scope) in result.safeguard_backup_paths.iter().zip(&plan.scopes) {
        let backup: InstanceBackupResult =
            serde_json::from_slice(&fs::read(Path::new(path).join("backup.json")).unwrap())
                .unwrap();
        let target =
            backup_restore_scope(&fixture.context.record, &fixture.context.target, &backup);
        assert_eq!(target, scope.target);
        restore_instance_backup_transaction(
            &fixture.context.record,
            &fixture.context.target,
            &fixture.context.record.summary.id,
            &backup.backup_id,
        )
        .unwrap();
    }
    assert_eq!(
        fs::read(fixture.context.target.join("old-player.dat")).unwrap(),
        b"old player"
    );
    assert_eq!(
        fs::read(plan.scopes[1].target.join("old-map")).unwrap(),
        b"original generated map"
    );
    let mut backup: InstanceBackupResult = serde_json::from_slice(
        &fs::read(Path::new(&result.safeguard_backup_paths[1]).join("backup.json")).unwrap(),
    )
    .unwrap();
    backup.saves_path = fixture.root.join("unowned").to_string_lossy().into_owned();
    assert_eq!(
        backup_restore_scope(&fixture.context.record, &fixture.context.target, &backup),
        fixture.context.target
    );
}

#[test]
fn save_import_server_plan_binds_secondary_target_and_native_selector() {
    let (fixture, mut plan) = grouped();
    let selector = fixture.context.record.config_dir.join("selector.json");
    fs::write(&selector, b"original selector").unwrap();
    plan.guards.push(selector.clone());
    let tree = io::inventory(&fixture.source, None).unwrap();
    let preview =
        server_transaction::preview(&fixture.context, &fixture.source, &tree, plan.clone())
            .unwrap();
    fs::write(plan.scopes[1].target.join("old-map"), b"changed map").unwrap();
    let import = || {
        server_transaction::import(
            &fixture.context,
            &fixture.source,
            &tree,
            plan.clone(),
            &preview.fingerprint,
            &publication::Publication::uncoordinated(),
            publish_new_directory,
        )
    };
    assert!(import().is_err());
    fs::write(
        plan.scopes[1].target.join("old-map"),
        b"original generated map",
    )
    .unwrap();
    fs::write(selector, b"changed selector").unwrap();
    assert!(import().is_err());
    assert_eq!(
        fs::read(fixture.context.target.join("old-player.dat")).unwrap(),
        b"old player"
    );
}

#[test]
fn save_import_server_plan_refuses_unowned_and_escaping_mappings() {
    let (fixture, mut plan) = grouped();
    plan.scopes[1].target = fixture.root.join("outside");
    assert!(validate(&fixture.context, &plan).is_err());
    plan.scopes.pop();
    plan.scopes[0].mappings[0].target = "../config".into();
    assert!(validate(&fixture.context, &plan).is_err());
    plan.scopes[0].mappings[0].target = "safe".into();
    plan.scopes[0].clear.push("../config".into());
    assert!(validate(&fixture.context, &plan).is_err());
}

#[test]
fn save_import_server_plan_nested_paths_read_back_with_retained_parent_spelling() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.context.target.join("Shared/Other")).unwrap();
    fs::write(
        fixture.context.target.join("Shared/Other/keep"),
        b"unselected world",
    )
    .unwrap();
    let mapped = if cfg!(windows) {
        "shared/New/Slot"
    } else {
        "Shared/New/Slot"
    };
    let scope = ServerScope::new(
        fixture.context.target.clone(),
        vec![ServerMapping {
            source: "region".into(),
            target: mapped.into(),
            replace: true,
        }],
    );
    let current = io::inventory(&fixture.context.target, None).unwrap();
    let projection = project(
        &scope,
        &fixture.source,
        &io::inventory(&fixture.source, None).unwrap(),
        Some(&current),
    )
    .unwrap();
    let staging = fixture.root.join("checked-staging");
    fs::create_dir(&staging).unwrap();
    write(&projection, &staging).unwrap();
    assert_eq!(
        fs::read(staging.join("Shared/New/Slot/r.0.0.mca")).unwrap(),
        b"new region"
    );
    assert_eq!(
        fs::read(staging.join("Shared/Other/keep")).unwrap(),
        b"unselected world"
    );
}
