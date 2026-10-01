use super::{creation, inventory};
use app_core::{InstanceDetails, InstanceProgramMode, InstanceStatus};
use app_modules::ModuleDescriptor;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;
const UNKNOWN_MOD_CONTENTS: &[u8] = b"unknown native seed probe mod";

#[derive(Clone)]
pub(super) struct SeedProbe {
    fixture: PathBuf,
    first_program: PathBuf,
    first_saves: PathBuf,
    baseline: BTreeMap<PathBuf, (u64, u64)>,
    corrupted_baseline: BTreeMap<PathBuf, (u64, u64)>,
    original_byte: u8,
    changed_entry: PathBuf,
    mod_sentinel: String,
    save_sentinel: String,
}

impl SeedProbe {
    pub(super) async fn prepare(
        first: &InstanceDetails,
        descriptor: &ModuleDescriptor,
        fixture: &Path,
    ) -> ProbeResult<Self> {
        let first = first.clone();
        let descriptor = descriptor.clone();
        let fixture = fixture.to_path_buf();
        tokio::task::spawn_blocking(move || {
            Self::prepare_checked(&first, &descriptor, &fixture).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("native seed preparation worker failed: {error}"))?
        .map_err(Into::into)
    }

    fn prepare_checked(
        first: &InstanceDetails,
        descriptor: &ModuleDescriptor,
        fixture: &Path,
    ) -> ProbeResult<Self> {
        let mode = creation::CreationMode::from_env(descriptor)?;
        if mode.acquisition != creation::Acquisition::Official
            || mode.instances != creation::InstanceMode::SecondPrivate
        {
            return Err(
                "native seed corruption requires official_acquisition and second_private".into(),
            );
        }
        let (program, saves) = checked_instance(first, descriptor, fixture)?;
        if !app_storage::library_program_is_pristine(&program, descriptor, None)? {
            return Err(
                "native seed probe requires an intact official package before pollution".into(),
            );
        }
        let baseline = inventory::program_files(descriptor, &program)?;
        let preferred = descriptor
            .install
            .as_ref()
            .and_then(|install| install.verification_path.as_deref())
            .map(Path::new);
        let changed_entry = preferred
            .filter(|path| baseline.get(*path).is_some_and(|value| value.0 > 0))
            .map(Path::to_path_buf)
            .or_else(|| {
                baseline
                    .iter()
                    .find(|(_, value)| value.0 > 0)
                    .map(|(path, _)| path.clone())
            })
            .ok_or("native seed probe needs a nonempty declared program entry")?;
        let changed_path = checked_path(fixture, &program.join(&changed_entry), false)?;
        // A new server need not have created its save tree yet. Only prepare
        // creates it, after checking the existing ancestor and missing suffix.
        fs::create_dir_all(&saves)?;
        checked_path(fixture, &saves, true)?;
        let nonce = uuid::Uuid::new_v4();
        let mod_sentinel = format!("native-unknown-mod-{nonce}.txt");
        let save_sentinel = format!("native-old-save-{nonce}.txt");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(program.join(&mod_sentinel))?
            .write_all(UNKNOWN_MOD_CONTENTS)?;
        drop(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(saves.join(&save_sentinel))?,
        );
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(changed_path)?;
        let mut byte = [0_u8; 1];
        file.read_exact(&mut byte)?;
        let original_byte = byte[0];
        byte[0] ^= 0xff;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&byte)?;
        file.sync_all()?;
        if file.metadata()?.len() != baseline[&changed_entry].0 {
            return Err("native seed probe unexpectedly changed program length".into());
        }
        drop(file);
        if app_storage::library_program_is_pristine(&program, descriptor, None)? {
            return Err(
                "native seed pollution was not detected by official package verification".into(),
            );
        }
        let corrupted_baseline = inventory::program_files(descriptor, &program)?;
        println!(
            "NATIVE_LIFECYCLE module={} phase=seed_polluted official_pristine_before=true official_pristine_after=false changed_entry={} unknown_mod={} empty_save={}",
            descriptor.summary.id,
            changed_entry.display(),
            mod_sentinel,
            save_sentinel
        );
        Ok(Self {
            fixture: fs::canonicalize(fixture)?,
            first_program: program,
            first_saves: saves,
            baseline,
            corrupted_baseline,
            original_byte,
            changed_entry,
            mod_sentinel,
            save_sentinel,
        })
    }

    pub(super) async fn verify(
        &self,
        second: &InstanceDetails,
        descriptor: &ModuleDescriptor,
    ) -> ProbeResult<()> {
        let probe = self.clone();
        let second = second.clone();
        let descriptor = descriptor.clone();
        tokio::task::spawn_blocking(move || {
            probe
                .verify_checked(&second, &descriptor)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("native seed verification worker failed: {error}"))?
        .map_err(Into::into)
    }

    fn verify_checked(
        &self,
        second: &InstanceDetails,
        descriptor: &ModuleDescriptor,
    ) -> ProbeResult<()> {
        self.verify_first_preserved()?;
        if inventory::program_files(descriptor, &self.first_program)? != self.corrupted_baseline {
            return Err(
                "native automatic repair changed the first instance's program bytes".into(),
            );
        }
        let (program, saves) = checked_instance(second, descriptor, &self.fixture)?;
        if program == self.first_program || saves == self.first_saves {
            return Err("native seed probe received the original instance roots".into());
        }
        if !app_storage::library_program_is_pristine(&program, descriptor, None)? {
            return Err("native second installation is not an intact official package".into());
        }
        if inventory::program_files(descriptor, &program)? != self.baseline {
            return Err(
                "native second installation did not restore original official program entry bytes"
                    .into(),
            );
        }
        let retained = creation::instance_root(second)?.join("installation-retained");
        let mut forbidden = vec![
            program.join(&self.mod_sentinel),
            saves.join(&self.save_sentinel),
            retained.join(&self.mod_sentinel),
        ];
        if let Ok(relative) = self.first_saves.strip_prefix(&self.first_program) {
            forbidden.push(retained.join(relative).join(&self.save_sentinel));
        }
        for path in forbidden {
            match fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
                Ok(_) => {
                    return Err(
                        "native second installation inherited an unknown mod or old save sentinel"
                            .into(),
                    );
                }
            }
        }
        println!(
            "NATIVE_LIFECYCLE module={} phase=seed_verified official_pristine=true restored_entry={} declared_program_entries={} unknown_mod=absent old_save=absent first_unknown_mod=preserved",
            descriptor.summary.id,
            self.changed_entry.display(),
            self.baseline.len()
        );
        Ok(())
    }

    pub(super) fn verify_first_preserved(&self) -> ProbeResult<()> {
        self.verify_unknown_mod_preserved()?;
        let changed = checked_path(
            &self.fixture,
            &self.first_program.join(&self.changed_entry),
            false,
        )?;
        let mut byte = [0u8; 1];
        fs::File::open(changed)?.read_exact(&mut byte)?;
        if byte[0] != (self.original_byte ^ 0xff) {
            return Err(
                "native automatic repair overwrote the first instance's modified byte".into(),
            );
        }
        let save = checked_path(
            &self.fixture,
            &self.first_saves.join(&self.save_sentinel),
            false,
        )?;
        if fs::metadata(save)?.len() != 0 {
            return Err("native lifecycle changed its first instance's old save sentinel".into());
        }
        Ok(())
    }

    pub(super) fn verify_unknown_mod_preserved(&self) -> ProbeResult<()> {
        let path = checked_path(
            &self.fixture,
            &self.first_program.join(&self.mod_sentinel),
            false,
        )?;
        let mut contents = Vec::new();
        fs::File::open(path)?
            .take(UNKNOWN_MOD_CONTENTS.len() as u64 + 1)
            .read_to_end(&mut contents)?;
        if contents != UNKNOWN_MOD_CONTENTS {
            return Err("native lifecycle changed its first instance's unknown mod bytes".into());
        }
        Ok(())
    }
}

fn checked_instance(
    instance: &InstanceDetails,
    descriptor: &ModuleDescriptor,
    fixture: &Path,
) -> ProbeResult<(PathBuf, PathBuf)> {
    if instance.summary.module_id != descriptor.summary.id
        || instance.active_run.is_some()
        || !matches!(instance.summary.status, InstanceStatus::Stopped)
    {
        return Err("native seed probe requires the selected unstarted instance".into());
    }
    let instance_root = creation::instance_root(instance)?;
    if checked_path(fixture, instance_root, true)? == fs::canonicalize(fixture)? {
        return Err("native seed probe requires an instance below the fixture root".into());
    }
    if app_storage::instance_program_mode(instance_root)? != InstanceProgramMode::Independent {
        return Err("native seed probe refuses shared programs".into());
    }
    Ok((
        checked_path(fixture, &creation::program_root(instance)?, true)?,
        checked_save_path(fixture, Path::new(&instance.saves_path))?,
    ))
}

/// Resolve a possibly absent save tree without creating it or following links.
fn checked_save_path(fixture: &Path, path: &Path) -> ProbeResult<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("native seed save path must be absolute without traversal".into());
    }
    let mut ancestor = path;
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(Component::Normal(name)) = ancestor.components().next_back() else {
                    return Err("native seed save path has an invalid missing suffix".into());
                };
                missing.push(name.to_os_string());
                ancestor = ancestor
                    .parent()
                    .ok_or("native seed save path has no existing ancestor")?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let mut resolved = checked_path(fixture, ancestor, true)?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    if resolved == fs::canonicalize(fixture)? {
        return Err("native seed save path must be below the fixture root".into());
    }
    Ok(resolved)
}

pub(super) fn checked_path(fixture: &Path, path: &Path, directory: bool) -> ProbeResult<PathBuf> {
    let fixture = fs::canonicalize(fixture)?;
    let canonical = fs::canonicalize(path)?;
    if !canonical.starts_with(&fixture) {
        return Err("native seed probe refuses paths outside its disposable fixture".into());
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("native seed probe refuses reparse points".into());
            }
        }
        if metadata.file_type().is_symlink()
            || (ancestor == path
                && (metadata.is_dir() != directory || (!directory && !metadata.is_file())))
        {
            return Err("native seed probe requires plain directories and files".into());
        }
        if fs::canonicalize(ancestor)? == fixture {
            break;
        }
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_probe_requires_original_unknown_mod_bytes_to_remain() {
        let root = std::env::temp_dir().join(format!("native-seed-mod-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let probe = SeedProbe {
            fixture: root.clone(),
            first_program: root.clone(),
            first_saves: root.join("saves"),
            baseline: BTreeMap::new(),
            corrupted_baseline: BTreeMap::new(),
            original_byte: 0,
            changed_entry: PathBuf::from("server.exe"),
            mod_sentinel: "unknown.txt".into(),
            save_sentinel: "save.txt".into(),
        };
        let path = root.join(&probe.mod_sentinel);
        fs::create_dir(&probe.first_saves).unwrap();
        fs::write(probe.first_saves.join(&probe.save_sentinel), b"").unwrap();
        fs::write(root.join(&probe.changed_entry), [0xff]).unwrap();
        fs::write(&path, UNKNOWN_MOD_CONTENTS).unwrap();
        probe.verify_first_preserved().unwrap();
        let mut changed = UNKNOWN_MOD_CONTENTS.to_vec();
        changed[0] ^= 0xff;
        fs::write(&path, changed).unwrap();
        assert!(probe.verify_first_preserved().is_err());
        fs::remove_file(&path).unwrap();
        assert!(probe.verify_first_preserved().is_err());
        assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn seed_probe_resolves_absent_saves_without_creating_them() {
        let root = std::env::temp_dir().join(format!("native-seed-path-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let absent = root.join("instance/Saves/new-world");
        let resolved = checked_save_path(&root, &absent).unwrap();
        assert_eq!(
            resolved,
            root.canonicalize()
                .unwrap()
                .join("instance/Saves/new-world")
        );
        assert!(!absent.exists(), "verification must not create a save tree");
        fs::create_dir_all(&resolved).unwrap();
        assert_eq!(checked_save_path(&root, &absent).unwrap(), resolved);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn seed_probe_rejects_absent_saves_outside_fixture_and_traversal() {
        let root =
            std::env::temp_dir().join(format!("native-seed-boundary-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let outside = root.with_extension("outside").join("Saves");
        assert!(checked_save_path(&root, &outside).is_err());
        assert!(checked_save_path(&root, &root.join("missing/../Saves")).is_err());
        assert!(!outside.exists());
        assert!(!root.join("missing").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
