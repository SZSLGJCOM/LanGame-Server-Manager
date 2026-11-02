use super::*;
use std::fs;

struct Fixture {
    base: PathBuf,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("archive-tree-{}", uuid::Uuid::new_v4()));
        let root = base.join("owned");
        fs::create_dir_all(&root).unwrap();
        Self { base, root }
    }

    fn identity(&self) -> String {
        crate::instance_archive_files::identity(&self.root)
            .unwrap()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.base).expect("remove only the owned tree fixture");
    }
}

#[test]
fn deletion_covers_wide_nested_unicode_and_empty_directories() {
    let fixture = Fixture::new();
    for branch in ["branch-a", "branch-b", "世界"] {
        let directory = fixture.root.join(branch).join("nested");
        fs::create_dir_all(directory.join("empty")).unwrap();
        for index in 0..400 {
            fs::write(directory.join(format!("file-{index}.dat")), b"owned bytes").unwrap();
        }
    }
    let outside = fixture.base.join("outside.sentinel");
    fs::write(&outside, b"preserved").unwrap();
    let identity = fixture.identity();
    preflight_tree(&fixture.root, &identity).unwrap();
    purge_tree(&fixture.root, &identity).unwrap();
    assert!(!fixture.root.exists());
    assert_eq!(fs::read(outside).unwrap(), b"preserved");
}

#[test]
fn root_replaced_after_preflight_preserves_both_trees() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("world.dat"), b"original").unwrap();
    let tree = tree_entries(&fixture.root, &fixture.identity()).unwrap();
    let retired = fixture.base.join("original-root");
    fs::rename(&fixture.root, &retired).unwrap();
    fs::create_dir(&fixture.root).unwrap();
    fs::write(fixture.root.join("world.dat"), b"replacement").unwrap();
    let replacement = TreeNode::open_root(&fixture.root, true).unwrap();
    assert!(purge_entries(&tree, replacement).is_err());
    assert_eq!(fs::read(retired.join("world.dat")).unwrap(), b"original");
    assert_eq!(
        fs::read(fixture.root.join("world.dat")).unwrap(),
        b"replacement"
    );
}

#[test]
fn file_replaced_after_preflight_is_not_deleted() {
    let fixture = Fixture::new();
    let original = fixture.root.join("world.dat");
    fs::write(&original, b"original").unwrap();
    let tree = tree_entries(&fixture.root, &fixture.identity()).unwrap();
    let retired = fixture.base.join("original-file");
    fs::rename(&original, &retired).unwrap();
    fs::write(&original, b"replacement").unwrap();
    let root = TreeNode::open_root(&fixture.root, true).unwrap();
    assert!(purge_entries(&tree, root).is_err());
    assert_eq!(fs::read(retired).unwrap(), b"original");
    assert_eq!(fs::read(original).unwrap(), b"replacement");
}

#[test]
fn directory_replaced_after_preflight_preserves_its_descendants() {
    let fixture = Fixture::new();
    let original = fixture.root.join("world");
    fs::create_dir(&original).unwrap();
    fs::write(original.join("save.dat"), b"original").unwrap();
    let tree = tree_entries(&fixture.root, &fixture.identity()).unwrap();
    let retired = fixture.base.join("original-directory");
    fs::rename(&original, &retired).unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(original.join("save.dat"), b"replacement").unwrap();
    let root = TreeNode::open_root(&fixture.root, true).unwrap();
    assert!(purge_entries(&tree, root).is_err());
    assert_eq!(fs::read(retired.join("save.dat")).unwrap(), b"original");
    assert_eq!(fs::read(original.join("save.dat")).unwrap(), b"replacement");
}

#[test]
fn exceeding_the_depth_limit_preserves_all_owned_files() {
    let fixture = Fixture::new();
    let mut directory = fixture.root.clone();
    for _ in 0..=MAX_TREE_DEPTH {
        directory.push("d");
    }
    fs::create_dir_all(&directory).unwrap();
    fs::write(fixture.root.join("sentinel.dat"), b"preserved").unwrap();
    let result = purge_tree(&fixture.root, &fixture.identity());
    assert!(result.is_err());
    assert_eq!(
        fs::read(fixture.root.join("sentinel.dat")).unwrap(),
        b"preserved"
    );
    assert!(directory.is_dir());
}

#[cfg(windows)]
#[test]
fn readonly_file_is_removed_through_the_verified_handle() {
    let fixture = Fixture::new();
    let path = fixture.root.join("readonly.dat");
    fs::write(&path, b"owned").unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    purge_tree(&fixture.root, &fixture.identity()).unwrap();
    assert!(!fixture.root.exists());
}
