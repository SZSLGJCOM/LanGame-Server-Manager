use super::*;
use std::fs;
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    profile: Profile,
    files: BTreeMap<String, Vec<u8>>,
}

impl Fixture {
    fn new(module: &str) -> Self {
        let root = std::env::temp_dir().join(format!("lgsm-loader-start-{}", uuid::Uuid::new_v4()));
        let profile = Profile::get(module).unwrap();
        let win64 = profile.win64(&root);
        fs::create_dir_all(&win64).unwrap();
        fs::write(
            win64.join(profile.executable),
            b"unknown game; never executed",
        )
        .unwrap();
        if module == "conanexiles" {
            fs::write(win64.join("Dreamworld.dll"), b"unknown provider").unwrap();
        }
        let files = archive::expected(profile)
            .keys()
            .map(|name| (name.clone(), format!("owned fixture: {name}").into_bytes()))
            .collect();
        let fixture = Self {
            root,
            profile,
            files,
        };
        fixture.publish();
        fixture
    }

    fn path(&self, path: &str) -> PathBuf {
        self.profile.win64(&self.root).join(path)
    }
    fn publish(&self) {
        installation::publish_fixture(
            &self.profile.win64(&self.root),
            self.profile,
            &self.files,
            publish,
        )
        .unwrap();
    }
    fn start(&self) -> Result<StartupState, String> {
        prepare_startup(&self.root, self.profile.id)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn unsupported_loader_start_disables_only_the_owned_activation_and_can_restore_it() {
    for module in [
        "abioticfactor",
        "nightingale",
        "conanexiles",
        "scum",
        "windrose",
    ] {
        let fixture = Fixture::new(module);
        let owner = fs::read(fixture.path(".langame-ue4ss-loader.json")).unwrap();
        assert_eq!(
            fixture.start().unwrap(),
            StartupState::Unavailable,
            "{module}"
        );
        assert_eq!(
            fixture.start().unwrap(),
            StartupState::Unavailable,
            "{module} repeat"
        );
        assert!(!fixture.path(fixture.profile.proxy).exists());
        for (name, bytes) in &fixture.files {
            if name != fixture.profile.proxy {
                assert_eq!(&fs::read(fixture.path(name)).unwrap(), bytes);
            }
        }
        assert_eq!(
            fs::read(fixture.path(".langame-ue4ss-loader.json")).unwrap(),
            owner
        );
        fixture.publish();
        assert_eq!(
            fs::read(fixture.path(fixture.profile.proxy)).unwrap(),
            fixture.files[fixture.profile.proxy]
        );
        assert_eq!(
            fs::read(fixture.path(".langame-ue4ss-loader.json")).unwrap(),
            owner
        );
    }
}

#[test]
fn unsupported_loader_start_recovers_an_owned_pending_proxy() {
    let fixture = Fixture::new("abioticfactor");
    let owner_path = fixture.path(".langame-ue4ss-loader.json");
    let mut owner: serde_json::Value =
        serde_json::from_slice(&fs::read(&owner_path).unwrap()).unwrap();
    let next_proxy = b"pending owned proxy";
    owner["pending_files"] = owner["files"].clone();
    owner["pending_files"][fixture.profile.proxy] = hash(next_proxy).into();
    let intent = serde_json::to_vec(&owner).unwrap();
    fs::write(&owner_path, &intent).unwrap();
    fs::write(fixture.path(fixture.profile.proxy), next_proxy).unwrap();
    assert_eq!(fixture.start().unwrap(), StartupState::Unavailable);
    assert!(!fixture.path(fixture.profile.proxy).exists());
    assert_eq!(fs::read(owner_path).unwrap(), intent);
    fixture.publish();
    assert_eq!(
        fs::read(fixture.path(fixture.profile.proxy)).unwrap(),
        fixture.files[fixture.profile.proxy]
    );
}

#[test]
fn unsupported_loader_start_preserves_conflicting_loaders_and_operator_mods() {
    for scenario in [
        "proxy",
        "core",
        "receipt",
        "operator_mod",
        "unowned_save_mod",
        "override",
        "other_proxy",
    ] {
        let fixture = Fixture::new("abioticfactor");
        match scenario {
            "proxy" => fs::write(fixture.path(fixture.profile.proxy), b"external proxy").unwrap(),
            "core" => fs::write(fixture.path("ue4ss/UE4SS.dll"), b"external core").unwrap(),
            "receipt" => fs::write(
                fixture.path(".langame-ue4ss-loader.json"),
                b"invalid receipt",
            )
            .unwrap(),
            "operator_mod" | "unowned_save_mod" => {
                let name = if scenario == "operator_mod" {
                    "OperatorMod"
                } else {
                    "LgsmWorldSave"
                };
                let directory = fixture.path(&format!("ue4ss/Mods/{name}"));
                fs::create_dir_all(&directory).unwrap();
                fs::write(directory.join("enabled.txt"), b"").unwrap();
            }
            "override" => fs::write(
                fixture.path("ue4ss/UE4SS-settings.ini"),
                b"[Overrides]\nModsFolderPath=../OperatorMods\n",
            )
            .unwrap(),
            "other_proxy" => fs::write(fixture.path("winmm.dll"), b"external loader").unwrap(),
            _ => unreachable!(),
        }
        let proxy = fs::read(fixture.path(fixture.profile.proxy)).unwrap();
        let owner = fs::read(fixture.path(".langame-ue4ss-loader.json")).unwrap();
        assert!(fixture.start().is_err(), "{scenario}");
        assert_eq!(
            fs::read(fixture.path(fixture.profile.proxy)).unwrap(),
            proxy,
            "{scenario}"
        );
        assert_eq!(
            fs::read(fixture.path(".langame-ue4ss-loader.json")).unwrap(),
            owner,
            "{scenario}"
        );
    }
}
