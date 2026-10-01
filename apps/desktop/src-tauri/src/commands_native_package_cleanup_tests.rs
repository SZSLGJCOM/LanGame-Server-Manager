use super::*;
use std::os::windows::process::CommandExt;

const CACHE: &str = "AppData/Local/Microsoft/Windows/INetCache";

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    links: Vec<PathBuf>,
}

impl Fixture {
    fn new() -> Self {
        let base =
            std::env::temp_dir().join(format!("lg-cache-unlink-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&base).unwrap();
        let root = base.join("owned");
        fs::create_dir(&root).unwrap();
        Self {
            base,
            root,
            links: Vec::new(),
        }
    }

    fn cache(&self) -> PathBuf {
        self.root.join("profile").join(CACHE)
    }

    fn junction(&mut self, link: &Path, target: &Path) {
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(target.to_string_lossy().replace('/', "\\"))
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(output.status.success(), "create owned junction: {output:?}");
        self.links.push(link.to_path_buf());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for link in self.links.iter().rev() {
            if fs::symlink_metadata(link).is_ok() {
                fs::remove_dir(link).expect("unlink only the test-created junction");
            }
        }
        fs::remove_dir_all(&self.base).expect("remove the exclusively created test fixture");
    }
}

#[test]
fn native_package_cache_unlinks_internal_junction_without_touching_target() {
    let mut fixture = Fixture::new();
    let target = fixture.cache().join("IE");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("sentinel"), b"owned cache data").unwrap();
    let link = fixture.cache().join("Content.IE5");
    fixture.junction(&link, &target);

    remove_profile_cache_junction(&fixture.root).unwrap();

    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(
        fs::read(target.join("sentinel")).unwrap(),
        b"owned cache data"
    );
}

#[test]
fn native_package_cache_unlinks_canonical_target_without_touching_target() {
    let mut fixture = Fixture::new();
    let target = fixture.cache().join("IE");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("sentinel"), b"owned cache data").unwrap();
    let link = fixture.cache().join("Content.IE5");
    let canonical_target = fs::canonicalize(&target).unwrap();
    fixture.junction(&link, &canonical_target);

    remove_profile_cache_junction(&fixture.root).unwrap();

    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(
        fs::read(target.join("sentinel")).unwrap(),
        b"owned cache data"
    );
}

#[test]
fn native_package_cache_rejects_external_target_without_unlinking() {
    let mut fixture = Fixture::new();
    fs::create_dir_all(fixture.cache().join("IE")).unwrap();
    let outside = fixture.base.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"outside data").unwrap();
    let link = fixture.cache().join("Content.IE5");
    fixture.junction(&link, &outside);

    let error = remove_profile_cache_junction(&fixture.root).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(fs::read_link(&link).is_ok());
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside data");
}

#[test]
fn native_package_cache_rejects_redirected_target_directory() {
    let mut fixture = Fixture::new();
    fs::create_dir_all(fixture.cache()).unwrap();
    let outside = fixture.base.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"outside data").unwrap();
    let target = fixture.cache().join("IE");
    fixture.junction(&target, &outside);
    let link = fixture.cache().join("Content.IE5");
    fixture.junction(&link, &target);

    assert!(remove_profile_cache_junction(&fixture.root).is_err());
    assert!(fs::read_link(&link).is_ok());
    assert!(fs::read_link(&target).is_ok());
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"outside data");
}

#[test]
fn native_package_cache_rejects_junction_ancestor_without_touching_profile() {
    let mut fixture = Fixture::new();
    let outside = fixture.base.join("outside-profile");
    let target = outside.join(CACHE).join("IE");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("sentinel"), b"outside profile").unwrap();
    let link = outside.join(CACHE).join("Content.IE5");
    fixture.junction(&link, &target);
    let profile = fixture.root.join("profile");
    fixture.junction(&profile, &outside);

    assert!(remove_profile_cache_junction(&fixture.root).is_err());
    assert!(fs::read_link(&link).is_ok());
    assert!(fs::read_link(&profile).is_ok());
    assert_eq!(
        fs::read(target.join("sentinel")).unwrap(),
        b"outside profile"
    );
}

#[test]
fn native_package_cache_leaves_plain_directory_for_normal_package_removal() {
    let fixture = Fixture::new();
    let plain = fixture.cache().join("Content.IE5");
    fs::create_dir_all(&plain).unwrap();
    fs::write(plain.join("sentinel"), b"ordinary directory").unwrap();

    remove_profile_cache_junction(&fixture.root).unwrap();

    assert_eq!(
        fs::read(plain.join("sentinel")).unwrap(),
        b"ordinary directory"
    );
}

#[test]
fn native_package_cache_is_retained_until_process_shutdown_is_confirmed() {
    let mut fixture = Fixture::new();
    let target = fixture.cache().join("IE");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("sentinel"), b"running process data").unwrap();
    let link = fixture.cache().join("Content.IE5");
    fixture.junction(&link, &target);

    drop(NativePackage {
        root: fixture.root.clone(),
        install_root: fixture.root.join("fixture"),
        copied_bytes: 0,
        cleanup_allowed: Arc::new(AtomicBool::new(false)),
    });

    assert!(fs::read_link(&link).is_ok());
    assert_eq!(
        fs::read(target.join("sentinel")).unwrap(),
        b"running process data"
    );
}

#[test]
fn native_package_cache_recovers_from_native_junction_list_directory_denial() {
    let mut fixture = Fixture::new();
    let target = fixture.cache().join("IE");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("sentinel"), b"unchanged target").unwrap();
    let link = fixture.cache().join("Content.IE5");
    fixture.junction(&link, &target);
    let read_acl = |path: &Path| {
        let output = std::process::Command::new("icacls.exe")
            .arg(path)
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(output.status.success(), "read owned target ACL");
        output.stdout
    };
    let before_acl = read_acl(&target);
    // /L applies the explicit deny to this test-created junction itself;
    // no inheritance flags or recursive ACL operations affect its target.
    let denied = std::process::Command::new("icacls.exe")
        .arg(&link)
        .args(["/deny", "*S-1-1-0:(RD)", "/L"])
        .creation_flags(0x0800_0000)
        .output()
        .unwrap();
    assert!(denied.status.success(), "set only the owned junction's ACL");
    assert!(
        before_acl == read_acl(&target),
        "target ACL must not change"
    );
    assert_eq!(
        fs::read(target.join("sentinel")).unwrap(),
        b"unchanged target"
    );
    // Opening the junction for recursive traversal is the same denied access
    // used when remove_dir_all reaches it from the package root.
    let error = fs::remove_dir_all(&link).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(error.raw_os_error(), Some(5));

    drop(NativePackage {
        root: fixture.root.clone(),
        install_root: fixture.root.join("fixture"),
        copied_bytes: 0,
        cleanup_allowed: Arc::new(AtomicBool::new(true)),
    });

    assert!(
        !fixture.root.exists(),
        "confirmed stopped package must be removed"
    );
}
