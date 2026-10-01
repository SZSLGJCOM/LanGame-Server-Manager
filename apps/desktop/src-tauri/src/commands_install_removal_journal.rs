use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use app_storage::program_removal_files as files;
use serde::{Deserialize, Serialize};

const MAX_JOURNAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_RETAINED_PATHS: usize = 4_096;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RetainedPath {
    relative: PathBuf,
    identity: String,
    directory: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScaffoldDirectory {
    relative: PathBuf,
    identity: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    version: u32,
    pub operation_id: String,
    pub module_id: String,
    pub install_id: i64,
    pub source: PathBuf,
    staged: PathBuf,
    scaffold: PathBuf,
    source_identity: String,
    parent_identity: String,
    retained: Vec<RetainedPath>,
    scaffold_directories: Vec<ScaffoldDirectory>,
    marker_identity: Option<String>,
}

fn identity(path: &Path, directory: bool) -> Result<Option<String>, String> {
    files::identity(path, directory).map_err(|error| error.to_string())
}

fn required_identity(path: &Path, directory: bool) -> Result<String, String> {
    identity(path, directory)?.ok_or_else(|| format!("卸载恢复路径不存在：{}", path.display()))
}

fn matches_identity(path: &Path, expected: &str, directory: bool) -> Result<bool, String> {
    match identity(path, directory)? {
        None => Ok(false),
        Some(actual) if actual == expected => Ok(true),
        Some(_) => Err(format!(
            "卸载路径的文件身份已改变，已保留现有内容：{}",
            path.display()
        )),
    }
}

fn absent(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(format!("无法检查卸载路径 {}：{error}", path.display())),
        Ok(_) => Ok(false),
    }
}

impl Journal {
    /// The source is still untouched when this plan and its empty retention
    /// scaffold are ready. The caller must persist the plan before stage().
    pub(super) fn prepare(
        record: &app_storage::ProgramInstallRecord,
        retained: Vec<PathBuf>,
    ) -> Result<Self, String> {
        if retained.len() > MAX_RETAINED_PATHS {
            return Err("卸载保留路径数量超过安全上限".into());
        }
        let source =
            files::normalize_path(&record.install_root).map_err(|error| error.to_string())?;
        let parent = source.parent().ok_or("卸载目录缺少父目录")?;
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("卸载目录名称无效")?;
        let operation_id = uuid::Uuid::new_v4().to_string();
        let staged = parent.join(format!(".{name}.uninstall-{operation_id}"));
        let scaffold = parent.join(format!(".{name}.uninstall-retained-{operation_id}"));
        if !absent(&staged)? || !absent(&scaffold)? {
            return Err("卸载隔离目录已存在，已拒绝覆盖".into());
        }
        let mut paths = Vec::new();
        for relative in retained {
            valid_relative(&relative)?;
            if relative == Path::new(app_steamcmd::RETAINED_INSTALL_DATA_MARKER) {
                return Err("保留数据占用了卸载状态标记，已拒绝移动程序目录".into());
            }
            let path = source.join(&relative);
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            let directory = metadata.is_dir();
            paths.push(RetainedPath {
                relative,
                identity: required_identity(&path, directory)?,
                directory,
            });
        }
        let mut journal = Self {
            version: 1,
            operation_id,
            module_id: record.module_id.clone(),
            install_id: record.id,
            staged,
            scaffold,
            source_identity: required_identity(&source, true)?,
            parent_identity: required_identity(parent, true)?,
            source,
            retained: paths,
            scaffold_directories: Vec::new(),
            marker_identity: None,
        };
        if let Err(error) = journal.prepare_scaffold() {
            return match journal.remove_scaffold(&journal.scaffold) {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!("{error}；保留目录准备清理失败：{cleanup}")),
            };
        }
        if let Err(error) = journal.encode() {
            return match journal.remove_scaffold(&journal.scaffold) {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!("{error}；保留目录准备清理失败：{cleanup}")),
            };
        }
        Ok(journal)
    }

    fn prepare_scaffold(&mut self) -> Result<(), String> {
        if self.retained.is_empty() {
            return Ok(());
        }
        let mut directories = BTreeSet::new();
        directories.insert(PathBuf::new());
        for retained in &self.retained {
            for parent in retained.relative.ancestors().skip(1) {
                directories.insert(parent.to_owned());
            }
        }
        let mut directories: Vec<_> = directories.into_iter().collect();
        if directories.len() > MAX_RETAINED_PATHS {
            return Err("卸载保留目录数量超过安全上限".into());
        }
        directories.sort_by_key(|path| path.components().count());
        for relative in directories {
            let directory = self.scaffold.join(&relative);
            fs::create_dir(&directory).map_err(|error| error.to_string())?;
            self.scaffold_directories.push(ScaffoldDirectory {
                relative,
                identity: required_identity(&directory, true)?,
            });
        }
        app_steamcmd::mark_retained_install_data(&self.scaffold)
            .map_err(|error| error.to_string())?;
        self.marker_identity = Some(required_identity(
            &self
                .scaffold
                .join(app_steamcmd::RETAINED_INSTALL_DATA_MARKER),
            false,
        )?);
        Ok(())
    }

    pub(super) fn encode(&self) -> Result<String, String> {
        let json = serde_json::to_string(self).map_err(|error| error.to_string())?;
        if json.len() > MAX_JOURNAL_BYTES {
            return Err("卸载恢复日志超过安全上限".into());
        }
        Ok(json)
    }

    pub(super) fn decode(json: &str) -> Result<Self, String> {
        if json.len() > MAX_JOURNAL_BYTES {
            return Err("卸载恢复日志超过安全上限".into());
        }
        let journal: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        if journal.version != 1
            || uuid::Uuid::parse_str(&journal.operation_id)
                .ok()
                .map(|id| id.to_string())
                .as_deref()
                != Some(&journal.operation_id)
            || journal.retained.len() > MAX_RETAINED_PATHS
            || journal.scaffold_directories.len() > MAX_RETAINED_PATHS
        {
            return Err("卸载恢复日志的版本、操作标识或路径数量无效".into());
        }
        let source = files::normalize_path(&journal.source).map_err(|error| error.to_string())?;
        if source != journal.source {
            return Err("卸载恢复日志的源路径不是规范路径".into());
        }
        let parent = source.parent().ok_or("卸载恢复路径缺少父目录")?;
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("卸载目录名称无效")?;
        if journal.staged != parent.join(format!(".{name}.uninstall-{}", journal.operation_id))
            || journal.scaffold
                != parent.join(format!(
                    ".{name}.uninstall-retained-{}",
                    journal.operation_id
                ))
        {
            return Err("卸载恢复日志的隔离路径不属于本次操作".into());
        }
        let mut relatives = BTreeSet::new();
        for retained in &journal.retained {
            valid_relative(&retained.relative)?;
            let key = folded(&retained.relative);
            if !relatives.insert(key.clone())
                || relatives.iter().any(|other| {
                    other != &key
                        && (key.starts_with(&format!("{other}/"))
                            || other.starts_with(&format!("{key}/")))
                })
            {
                return Err("卸载恢复日志包含重复或重叠的保留路径".into());
            }
        }
        let mut expected = BTreeSet::new();
        if !journal.retained.is_empty() {
            expected.insert(PathBuf::new());
        }
        for retained in &journal.retained {
            expected.extend(retained.relative.ancestors().skip(1).map(Path::to_owned));
        }
        let actual: BTreeSet<_> = journal
            .scaffold_directories
            .iter()
            .map(|directory| directory.relative.clone())
            .collect();
        if actual != expected
            || actual.len() != journal.scaffold_directories.len()
            || journal.marker_identity.is_some() != !journal.retained.is_empty()
        {
            return Err("卸载恢复日志的保留目录清单无效".into());
        }
        journal.check_parent()?;
        Ok(journal)
    }

    fn check_parent(&self) -> Result<(), String> {
        if matches_identity(
            self.source.parent().ok_or("卸载目录缺少父目录")?,
            &self.parent_identity,
            true,
        )? {
            Ok(())
        } else {
            Err("卸载目录的父目录已丢失".into())
        }
    }

    fn scaffold_identity(&self) -> Option<&str> {
        self.scaffold_directories
            .iter()
            .find(|entry| entry.relative.as_os_str().is_empty())
            .map(|entry| entry.identity.as_str())
    }

    pub(super) fn stage(&self) -> Result<(), String> {
        self.check_parent()?;
        files::preflight_directory(&self.source, &self.source_identity)
            .map_err(|error| error.to_string())?;
        move_path(&self.source, &self.staged, &self.source_identity, true)?;
        if let Some(expected) = self.scaffold_identity() {
            for retained in &self.retained {
                move_path(
                    &self.staged.join(&retained.relative),
                    &self.scaffold.join(&retained.relative),
                    &retained.identity,
                    retained.directory,
                )?;
            }
            move_path(&self.scaffold, &self.source, expected, true)?;
        }
        Ok(())
    }

    pub(super) fn rollback(&self) -> Result<(), String> {
        self.check_parent()?;
        if absent(&self.staged)? {
            if !matches_identity(&self.source, &self.source_identity, true)? {
                return Err("原程序与卸载隔离目录均不存在，恢复记录已保留".into());
            }
            return self.remove_scaffold(&self.scaffold);
        }
        if !matches_identity(&self.staged, &self.source_identity, true)? {
            return Err("卸载隔离目录不存在".into());
        }
        let retention = if let Some(expected) = self.scaffold_identity() {
            if !absent(&self.source)? {
                if !absent(&self.scaffold)? {
                    return Err("卸载保留目录出现两个位置，已拒绝覆盖".into());
                }
                matches_identity(&self.source, expected, true)?;
                Some(&self.source)
            } else if !absent(&self.scaffold)? {
                matches_identity(&self.scaffold, expected, true)?;
                Some(&self.scaffold)
            } else {
                None
            }
        } else {
            if !absent(&self.source)? {
                return Err("原安装路径已被其他写入者重新创建".into());
            }
            None
        };
        for retained in &self.retained {
            let original = self.staged.join(&retained.relative);
            let saved = retention.map(|root| root.join(&retained.relative));
            let in_original = matches_identity(&original, &retained.identity, retained.directory)?;
            let in_saved = saved
                .as_ref()
                .map(|path| matches_identity(path, &retained.identity, retained.directory))
                .transpose()?
                .unwrap_or(false);
            if in_original == in_saved {
                return Err(format!(
                    "保留数据位置冲突或丢失：{}",
                    retained.relative.display()
                ));
            }
            if let Some(saved) = saved.filter(|_| in_saved) {
                move_path(&saved, &original, &retained.identity, retained.directory)?;
            }
        }
        if let Some(retention) = retention {
            self.remove_scaffold(retention)?;
        }
        move_path(&self.staged, &self.source, &self.source_identity, true)
    }

    fn remove_scaffold(&self, root: &Path) -> Result<(), String> {
        if absent(root)? {
            return Ok(());
        }
        if let Some(expected) = &self.marker_identity {
            let marker = root.join(app_steamcmd::RETAINED_INSTALL_DATA_MARKER);
            if matches_identity(&marker, expected, false)? {
                if !app_steamcmd::has_retained_install_data(root) {
                    return Err("卸载保留标记内容已被修改".into());
                }
                files::remove_file(&marker, expected).map_err(|error| error.to_string())?;
            }
        }
        let mut directories: Vec<_> = self.scaffold_directories.iter().collect();
        directories.sort_by_key(|entry| std::cmp::Reverse(entry.relative.components().count()));
        for directory in directories {
            let path = root.join(&directory.relative);
            if matches_identity(&path, &directory.identity, true)? {
                files::remove_empty_directory(&path, &directory.identity)
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    pub(super) fn finish_committed(&self) -> Result<(), String> {
        self.check_parent()?;
        if !absent(&self.scaffold)? {
            return Err("已提交卸载仍有未发布的保留目录，已保留恢复记录".into());
        }
        if matches_identity(&self.staged, &self.source_identity, true)? {
            retry_mutation(|| files::purge_directory(&self.staged, &self.source_identity))?;
        }
        Ok(())
    }

    pub(super) fn preserved_data_paths(&self) -> Vec<String> {
        self.retained
            .iter()
            .map(|entry| {
                self.source
                    .join(&entry.relative)
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }
}

fn valid_relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err("卸载保留路径不是安全相对路径".into());
    }
    Ok(())
}

fn folded(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

fn move_path(from: &Path, to: &Path, expected: &str, directory: bool) -> Result<(), String> {
    retry_mutation(|| files::move_path(from, to, expected, directory))
}

/// Match the existing uninstall rename grace period without weakening identity
/// checks: each attempt reopens and verifies the expected filesystem object.
fn retry_mutation(
    mut operation: impl FnMut() -> Result<(), app_storage::StorageError>,
) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        operation().map_err(|error| error.to_string())
    }
    #[cfg(windows)]
    {
        let started = std::time::Instant::now();
        loop {
            match operation() {
                Ok(()) => return Ok(()),
                Err(error) => {
                    #[cfg(windows)]
                    {
                        let transient = match &error {
                            app_storage::StorageError::ReadPath { source, .. }
                            | app_storage::StorageError::MovePath { source, .. }
                            | app_storage::StorageError::DeletePath { source, .. } => {
                                matches!(source.raw_os_error(), Some(5 | 32 | 33))
                            }
                            _ => false,
                        };
                        let remaining =
                            std::time::Duration::from_secs(3).saturating_sub(started.elapsed());
                        if transient && !remaining.is_zero() {
                            std::thread::sleep(
                                std::time::Duration::from_millis(100).min(remaining),
                            );
                            continue;
                        }
                    }
                    return Err(error.to_string());
                }
            }
        }
    }
}
