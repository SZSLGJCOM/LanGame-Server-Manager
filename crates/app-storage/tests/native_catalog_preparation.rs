use app_storage::{StoragePaths, bootstrap_storage_with_paths, list_instances};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn plain_path(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if !path.is_absolute() {
        return Err("native preparation requires absolute owned paths".into());
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink() {
            return Err("native preparation cannot cross symbolic links".into());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("native preparation cannot cross reparse points".into());
            }
        }
    }
    Ok(path.canonicalize()?)
}

#[test]
#[ignore = "initializes an explicitly owned disposable catalog for official package certification"]
fn initializes_isolated_native_catalog() -> TestResult {
    if env::var("LANGAME_ASSISTANT_LIVE").as_deref() != Ok("1") {
        return Err("native catalog preparation requires explicit live opt-in".into());
    }
    let runtime_root = plain_path(Path::new(
        &env::var_os("LANGAME_SMOKE_RUNTIME_ROOT")
            .ok_or("native preparation requires a disposable runtime root")?,
    ))?;
    let games_root = plain_path(Path::new(
        &env::var_os("LANGAME_DST_SMOKE_GAMES_ROOT")
            .ok_or("native preparation requires its owned game package")?,
    ))?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    if games_root == runtime_root
        || !games_root.starts_with(&runtime_root)
        || runtime_root.starts_with(&workspace)
        || workspace.starts_with(&runtime_root)
    {
        return Err("native preparation paths escaped their disposable package scope".into());
    }
    if let Ok(persistent) = Path::new(app_core::DEFAULT_LANGAME_DATA_ROOT).canonicalize()
        && (runtime_root.starts_with(&persistent) || persistent.starts_with(&runtime_root))
    {
        return Err("native preparation cannot use persistent LanGame data".into());
    }
    plain_path(
        &games_root.join("dontstarve/bin64/dontstarve_dedicated_server_nullrenderer_x64.exe"),
    )?;
    let steamcmd_root = plain_path(&runtime_root.join("cmd"))?;
    plain_path(&steamcmd_root.join("steamcmd.exe"))?;
    let receipt_path = runtime_root.join("preparation-receipt.json");
    if receipt_path.try_exists()? {
        return Err("native preparation receipt already exists; refusing duplicate setup".into());
    }
    let profile_root = runtime_root.join(format!("prep-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&profile_root)?;
    let app_data = profile_root.join("localappdata/LanGame/ServerManager");
    let paths = StoragePaths {
        app_data_root: app_data.clone(),
        settings_path: app_data.join("settings.json"),
        database_path: app_data.join("db/lgs.db"),
        logs_root: app_data.join("logs"),
        modules_root: workspace.join("modules"),
        migrations_root: workspace.join("migrations"),
        steamcmd_root,
        games_root,
        instances_root: profile_root.join("i"),
        archives_root: profile_root.join("archives"),
    };
    // Production creates a new database and its real migration history. Touching
    // an empty file would correctly trigger the existing-database refusal.
    let storage = bootstrap_storage_with_paths(paths)?;
    let runtime = tokio::runtime::Runtime::new()?;
    if !runtime.block_on(list_instances(&storage.paths))?.is_empty() {
        return Err("native preparation did not produce an empty isolated catalog".into());
    }
    let receipt = serde_json::json!({"profileRoot":profile_root,
        "databasePath":storage.paths.database_path,"gamesRoot":storage.paths.games_root,
        "steamcmdRoot":storage.paths.steamcmd_root,"initialInstanceCount":0,"modelRequests":0});
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(receipt_path)?;
    std::io::Write::write_all(&mut output, &serde_json::to_vec_pretty(&receipt)?)?;
    println!("ASSISTANT_NATIVE_PREPARATION={receipt}");
    Ok(())
}
