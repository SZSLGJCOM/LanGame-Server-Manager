#![cfg(test)]

use super::*;
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};

/// Opt-in fault injection only into an official package owned by this test.
/// A same-length entry corruption must be repaired before a real game starts.
pub(super) struct EntryRepairProbe {
    root: PathBuf,
    original: BTreeMap<PathBuf, (u64, u64)>,
}

impl EntryRepairProbe {
    pub(super) fn prepare(
        instance: &InstanceDetails,
        descriptor: &ModuleDescriptor,
        fixture: &Path,
        cycle: &str,
    ) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        if cycle != "initial"
            || std::env::var("LANGAME_NATIVE_ENTRY_REPAIR").as_deref() != Ok("true")
        {
            return Ok(None);
        }
        if creation::CreationMode::from_env(descriptor)?.acquisition
            != creation::Acquisition::Official
            || app_core::InstanceProgramUpdatePolicy::from_settings_json(&instance.settings_json)?
                != app_core::InstanceProgramUpdatePolicy::Automatic
        {
            return Err("native entry repair requires an automatic official acquisition".into());
        }
        let root = seed_probe::checked_path(fixture, &creation::program_root(instance)?, true)?;
        if !app_storage::library_program_is_pristine(&root, descriptor, None)? {
            return Err("native entry repair requires a verified intact package".into());
        }
        let original = inventory::program_files(descriptor, &root)?;
        let relative = descriptor
            .install
            .as_ref()
            .and_then(|install| install.verification_path.as_deref())
            .ok_or("native entry repair requires a concrete verification file")?;
        if !original
            .get(Path::new(relative))
            .is_some_and(|entry| entry.0 > 0)
        {
            return Err("native repair entry must be a nonempty verified program file".into());
        }
        let entry = seed_probe::checked_path(fixture, &root.join(relative), false)?;
        let mut file = fs::OpenOptions::new().read(true).write(true).open(entry)?;
        let mut byte = [0_u8];
        file.read_exact(&mut byte)?;
        byte[0] ^= 0xff;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&byte)?;
        file.sync_all()?;
        drop(file);
        if app_storage::program_launch_files_match_baseline(&root, descriptor, None)? != Some(false)
        {
            return Err("native fault injection did not damage the program entry".into());
        }
        println!(
            "NATIVE_LIFECYCLE module={} phase=entry_corrupted same_length=true isolated=true",
            descriptor.summary.id
        );
        Ok(Some(Self { root, original }))
    }

    pub(super) fn verify(
        self,
        descriptor: &ModuleDescriptor,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if inventory::program_files(descriptor, &self.root)? != self.original
            || !app_storage::library_program_is_pristine(&self.root, descriptor, None)?
        {
            return Err(
                "native startup did not restore the official program before spawning".into(),
            );
        }
        println!(
            "NATIVE_LIFECYCLE module={} phase=entry_repaired official_bytes=verified game_process=started",
            descriptor.summary.id
        );
        Ok(())
    }
}
