//! Compile the fixed save adapters against their verified native loader SDKs.
use super::{
    FILES, PROTOCOL, Profile, SONS_MANIFEST, corekeeper_native_files, extra_sources, io, redloader,
    source,
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug, thiserror::Error)]
pub(super) enum CompileFailure {
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Local(String),
}

impl From<String> for CompileFailure {
    fn from(message: String) -> Self {
        Self::Local(message)
    }
}

impl From<&str> for CompileFailure {
    fn from(message: &str) -> Self {
        Self::Local(message.into())
    }
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "langame-native-save-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&path).map_err(|_| "Cannot create the save compiler workspace.")?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        for name in [
            "SaveProtocol.cs",
            "SaveFiles.cs",
            "NativeSave.cs",
            "SonsSaveCompletion.cs",
            "CoreKeeperSaveNative.cs",
            "0Harmony.dll",
            "manifest.json",
            "save.dll",
            "compiler.txt",
            "compiler.rsp",
        ] {
            let _ = std::fs::remove_file(self.0.join(name));
        }
        let _ = std::fs::remove_dir(&self.0);
    }
}

pub(super) fn compile(root: &Path, game: Profile) -> Result<Vec<u8>, CompileFailure> {
    let scratch = Scratch::new()?;
    for (name, text) in [
        ("SaveProtocol.cs", PROTOCOL),
        ("SaveFiles.cs", FILES),
        ("NativeSave.cs", source(game)),
        ("manifest.json", SONS_MANIFEST),
    ] {
        std::fs::write(scratch.0.join(name), text)
            .map_err(|_| "Cannot stage the fixed save extension source.")?;
    }
    for (name, text) in extra_sources(game) {
        std::fs::write(scratch.0.join(name), text)
            .map_err(|_| "Cannot stage the native save completion source.")?;
    }
    let references = references(root, game)?;
    let mut pins = Vec::new();
    for (reference, expected) in &references {
        pins.extend(io::pin_directories(
            reference.parent().ok_or("Invalid compiler reference")?,
        )?);
        let (pin, bytes) = io::pin_regular(reference, 64 * 1024 * 1024)?;
        if io::digest(&bytes) != *expected {
            return Err(
                "The fixed compiler reference was changed; no extension was compiled.".into(),
            );
        }
        pins.push(pin);
    }
    let compiler = PathBuf::from(std::env::var_os("WINDIR").ok_or_else(|| {
        CompileFailure::Unavailable(
            "Native save preparation requires Windows Framework csc.".into(),
        )
    })?)
    .join("Microsoft.NET/Framework64/v4.0.30319/csc.exe");
    let mut output = File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(scratch.0.join("compiler.txt"))
        .map_err(|_| "Cannot create the save compiler diagnostic file.")?;
    let response = compiler_response(&compiler, &scratch.0, game, &references)?;
    let response_file = scratch.0.join("compiler.rsp");
    std::fs::write(&response_file, response)
        .map_err(|error| format!("Cannot stage the fixed save compiler response file: {error}"))?;
    let mut command = Command::new(&compiler);
    // IL2CPP SDKs have hundreds of references. Keeping them in the response
    // file avoids Windows' 32,767-character process command-line limit. This
    // option itself must stay on the command line (Framework warning CS2023).
    command
        .arg("/noconfig")
        .arg(format!("@{}", response_file.display()));
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(output.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(output.try_clone().map_err(|e| e.to_string())?));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let result = run(&mut command, Duration::from_secs(30));
    let (diagnostics, truncated) = read_diagnostics(&mut output)?;
    if !diagnostics.is_empty() {
        eprintln!(
            "native-save compiler: {}",
            String::from_utf8_lossy(&diagnostics)
        );
    }
    if truncated {
        eprintln!("native-save compiler: diagnostics truncated after 64 KiB.");
    }
    result?;
    io::read_regular(&scratch.0.join("save.dll"), 1024 * 1024).map_err(Into::into)
}

fn read_diagnostics(file: &mut File) -> Result<(Vec<u8>, bool), CompileFailure> {
    // Retain the handle we created; a replaced path cannot redirect this read.
    // Verbose compiler errors must not turn an optional adapter into a startup failure.
    file.rewind().map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let truncated = bytes.len() > 65536;
    bytes.truncate(65536);
    Ok((bytes, truncated))
}

pub(super) fn run(command: &mut Command, timeout: Duration) -> Result<(), CompileFailure> {
    let mut child = command.spawn().map_err(|error| {
        let message =
            format!("Cannot start Windows Framework csc for the fixed save extension: {error}");
        if error.kind() == std::io::ErrorKind::NotFound {
            CompileFailure::Unavailable(message)
        } else {
            CompileFailure::Local(message)
        }
    })?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let cleanup = stop(&mut child);
                return Err(CompileFailure::Local(match cleanup {
                    Ok(()) => format!("Cannot observe the save extension compiler: {error}"),
                    Err(cleanup) => {
                        format!("Cannot observe the save extension compiler: {error}; {cleanup}")
                    }
                }));
            }
        }
        if Instant::now() >= deadline {
            stop(&mut child)?;
            return Err(CompileFailure::Unavailable(format!(
                "The save extension compiler exceeded {} seconds; no plugin was installed.",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if status.success() {
        Ok(())
    } else {
        Err(CompileFailure::Unavailable(format!(
            "The fixed save extension compiler exited with {status}; no plugin was installed. See the application compiler diagnostics."
        )))
    }
}

fn stop(child: &mut Child) -> Result<(), CompileFailure> {
    if let Err(error) = child.kill() {
        return match child.try_wait() {
            Ok(Some(_)) => Ok(()),
            _ => Err(CompileFailure::Local(format!(
                "Cannot stop the save extension compiler: {error}"
            ))),
        };
    }
    child.wait().map_err(|error| {
        CompileFailure::Local(format!("Cannot reap the save extension compiler: {error}"))
    })?;
    Ok(())
}

fn compiler_path(path: &Path) -> Result<String, String> {
    // Framework csc rejects verbatim paths even when the pinned file exists.
    // Simplify only equivalent paths at this process boundary, not the I/O guards.
    let text = dunce::simplified(path)
        .to_str()
        .ok_or("The save compiler path is not valid Unicode.")?;
    if text
        .chars()
        .any(|character| character == '"' || character.is_control())
    {
        return Err("The save compiler path contains an invalid response-file character.".into());
    }
    Ok(format!("\"{text}\""))
}

fn compiler_response(
    compiler: &Path,
    scratch: &Path,
    game: Profile,
    references: &[(PathBuf, String)],
) -> Result<Vec<u8>, String> {
    let mut arguments = [
        "/nologo",
        "/target:library",
        "/langversion:5",
        "/warnaserror+",
    ]
    .map(str::to_owned)
    .to_vec();
    if matches!(game.module, "vrising" | "sonsoftheforest") {
        arguments.push("/nostdlib+".into());
        arguments.push("/warnaserror-:1701".into());
    } else {
        for name in ["System.dll", "System.Core.dll"] {
            arguments.push(format!(
                "/reference:{}",
                compiler_path(
                    &compiler
                        .parent()
                        .ok_or("Invalid Framework compiler path")?
                        .join(name)
                )?
            ));
        }
    }
    arguments.push(format!(
        "/out:{}",
        compiler_path(&scratch.join("save.dll"))?
    ));
    for (reference, _) in references {
        arguments.push(format!("/reference:{}", compiler_path(reference)?));
    }
    if game.module == "sonsoftheforest" {
        arguments.push(format!(
            "/resource:{},LanGame.Sons.manifest.json",
            compiler_path(&scratch.join("manifest.json"))?
        ));
    }
    for name in ["SaveProtocol.cs", "SaveFiles.cs", "NativeSave.cs"] {
        arguments.push(compiler_path(&scratch.join(name))?);
    }
    for (name, _) in extra_sources(game) {
        arguments.push(compiler_path(&scratch.join(name))?);
    }
    Ok(format!("\u{feff}{}\r\n", arguments.join("\r\n")).into_bytes())
}

fn references(root: &Path, game: Profile) -> Result<Vec<(PathBuf, String)>, String> {
    if game.module == "barotrauma" {
        return Ok(Vec::new());
    }
    if game.module == "corekeeper" {
        let files = corekeeper_native_files(root, game)?;
        return Ok([
            ("PugMod.SDK.Runtime.dll", files[1].1),
            (
                "UnityEngine.CoreModule.dll",
                "d184eb633e67cd97d5a0eb0faf3a8f05bdfe17e56c3cef5595cf15c685030aa6",
            ),
            (
                "netstandard.dll",
                "6ae62e082dc494a2433984177f60ca4db5fae69b1f360a8b33754172b310b8c5",
            ),
        ]
        .into_iter()
        .map(|(name, hash)| {
            (
                root.join("CoreKeeperServer_Data/Managed").join(name),
                hash.into(),
            )
        })
        .collect());
    }
    let (runtime, sdk) = if game.module == "vrising" {
        ("dotnet", "BepInEx/core")
    } else {
        ("_Redloader/dotnet", "_Redloader/net6")
    };
    let mut references = Vec::new();
    let files: BTreeMap<String, String> = if game.module == "vrising" {
        #[derive(serde::Deserialize)]
        struct Loader {
            files: BTreeMap<String, String>,
        }
        serde_json::from_str::<Loader>(include_str!("../assets/mod-loaders/vrising.json"))
            .map_err(|_| "Invalid reviewed V Rising loader manifest.")?
            .files
    } else {
        redloader::files()?
    };
    for (path, hash) in &files {
        let relative = Path::new(path);
        let Some(name) = relative.file_name().and_then(|v| v.to_str()) else {
            continue;
        };
        if relative.parent() == Some(Path::new(runtime))
            && name.ends_with(".dll")
            && !name.ends_with(".Native.dll")
            && (name.starts_with("System.")
                || ["mscorlib.dll", "netstandard.dll", "Microsoft.CSharp.dll"].contains(&name))
        {
            references.push((root.join(path), hash.clone()));
        }
    }
    let libraries: &[&str] = if game.module == "vrising" {
        &[
            "BepInEx.Core.dll",
            "BepInEx.Unity.IL2CPP.dll",
            "0Harmony.dll",
            "Il2CppInterop.Runtime.dll",
            "Il2CppInterop.Common.dll",
        ]
    } else {
        &["RedLoader.dll", "SonsSdk.dll", "0Harmony.dll"]
    };
    for name in libraries {
        let relative = format!("{sdk}/{name}");
        references.push((
            root.join(&relative),
            files
                .get(&relative)
                .ok_or("The fixed loader SDK is incomplete.")?
                .clone(),
        ));
    }
    references.sort();
    Ok(references)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::commands_managed_save_control::profile;

    #[test]
    fn verbose_compiler_diagnostics_are_bounded_without_rejecting_the_result() {
        use std::io::Write;

        let scratch = Scratch::new().unwrap();
        let mut output = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(scratch.0.join("compiler.txt"))
            .unwrap();
        output.write_all(&vec![b'E'; 70000]).unwrap();
        let (bytes, truncated) = read_diagnostics(&mut output).unwrap();
        assert_eq!(bytes, vec![b'E'; 65536]);
        assert!(truncated);
        output.set_len(32).unwrap();
        assert_eq!(
            read_diagnostics(&mut output).unwrap(),
            (vec![b'E'; 32], false)
        );
    }

    #[cfg(windows)]
    #[test]
    fn compiler_exit_and_timeout_are_optional_but_invalid_executable_is_not() {
        use std::os::windows::process::CommandExt;

        let system = PathBuf::from(std::env::var_os("WINDIR").unwrap()).join("System32");
        let command = |arguments: &[&str]| {
            let mut command = Command::new(system.join("cmd.exe"));
            command
                .args(arguments)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x08000000);
            command
        };
        run(
            &mut command(&["/d", "/c", "exit 0"]),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(matches!(
            run(&mut command(&["/d", "/c", "exit 27"]), Duration::from_secs(5)),
            Err(CompileFailure::Unavailable(issue)) if issue.contains("27")
        ));

        let mut waiting = Command::new(system.join("WindowsPowerShell/v1.0/powershell.exe"));
        waiting
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000);
        assert!(matches!(
            run(&mut waiting, Duration::ZERO),
            Err(CompileFailure::Unavailable(issue)) if issue.contains("exceeded")
        ));

        let scratch = Scratch::new().unwrap();
        let invalid = scratch.0.join("save.dll");
        std::fs::write(&invalid, b"not an executable").unwrap();
        let mut invalid_command = Command::new(invalid);
        invalid_command.creation_flags(0x08000000);
        assert!(matches!(
            run(&mut invalid_command, Duration::from_secs(5)),
            Err(CompileFailure::Local(_))
        ));
    }

    #[test]
    fn compiler_response_preserves_long_reference_lists_and_quoted_unicode_paths() {
        let game = profile("vrising").unwrap();
        let root = PathBuf::from(format!(
            "C:/operator games/存档/{}/runtime",
            "x".repeat(140)
        ));
        let references = (0..200)
            .map(|index| {
                (
                    root.join(format!("dotnet/System.Reference{index}.dll")),
                    "fixed hash".into(),
                )
            })
            .collect::<Vec<_>>();
        let scratch = PathBuf::from("C:/compiler output/保存");
        let bytes = compiler_response(
            Path::new("C:/Windows/Framework/csc.exe"),
            &scratch,
            game,
            &references,
        )
        .unwrap();
        assert!(bytes.len() > 32767);
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.starts_with('\u{feff}'));
        assert!(
            !text.contains("/noconfig"),
            "Framework csc requires this option on its command line"
        );
        for (reference, _) in references {
            assert!(text.contains(&format!("/reference:\"{}\"\r\n", reference.display())));
        }
        assert!(text.contains(&format!(
            "/out:\"{}\"\r\n",
            scratch.join("save.dll").display()
        )));
        assert!(text.contains(&format!(
            "\"{}\"\r\n",
            scratch.join("NativeSave.cs").display()
        )));
        assert!(text.ends_with(&format!(
            "\"{}\"\r\n",
            scratch.join("NativeSave.cs").display()
        )));
        let sons = compiler_response(
            Path::new("C:/Windows/Framework/csc.exe"),
            &scratch,
            profile("sonsoftheforest").unwrap(),
            &[],
        )
        .unwrap();
        let sons = std::str::from_utf8(&sons).unwrap();
        assert!(sons.ends_with(&format!(
            "\"{}\"\r\n",
            scratch.join("SonsSaveCompletion.cs").display()
        )));
    }

    #[test]
    fn compiler_response_rejects_path_characters_that_could_inject_options() {
        for path in [
            "C:/path\n/unsafe.cs",
            "C:/path\r/unsafe.cs",
            "C:/path\"unsafe.cs",
            "C:/path\0unsafe.cs",
        ] {
            assert!(compiler_path(Path::new(path)).is_err(), "{path:?}");
        }
        assert_eq!(
            compiler_path(Path::new("C:/operator # notes/存档.dll")).unwrap(),
            "\"C:/operator # notes/存档.dll\""
        );
        #[cfg(windows)]
        {
            assert_eq!(
                compiler_path(Path::new(r"\\?\C:\operator games\存档.dll")).unwrap(),
                "\"C:\\operator games\\存档.dll\""
            );
            for path in [
                r"\\?\C:\operator games\trailing .",
                r"\\?\C:\operator games\COM1.dll",
                r"\\?\UNC\server\share\file.dll",
            ] {
                assert_eq!(
                    compiler_path(Path::new(path)).unwrap(),
                    format!("\"{path}\"")
                );
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn compiler_response_accepts_canonical_windows_reference_paths() {
        use std::os::windows::process::CommandExt;

        let compiler = PathBuf::from(std::env::var_os("WINDIR").unwrap())
            .join("Microsoft.NET/Framework64/v4.0.30319/csc.exe");
        let reference =
            std::fs::canonicalize(compiler.parent().unwrap().join("System.Xml.dll")).unwrap();
        assert!(reference.to_string_lossy().starts_with(r"\\?\"));
        let scratch = Scratch::new().unwrap();
        for (name, text) in [
            ("SaveProtocol.cs", ""),
            ("SaveFiles.cs", ""),
            (
                "NativeSave.cs",
                "public class NativeSave { public System.Xml.XmlElement Value; }",
            ),
        ] {
            std::fs::write(scratch.0.join(name), text).unwrap();
        }
        let response = compiler_response(
            &compiler,
            &std::fs::canonicalize(&scratch.0).unwrap(),
            profile("barotrauma").unwrap(),
            &[(reference, String::new())],
        )
        .unwrap();
        let response_file = scratch.0.join("compiler.rsp");
        std::fs::write(&response_file, response).unwrap();
        let mut command = Command::new(compiler);
        command
            .args(["/noconfig", &format!("@{}", response_file.display())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000);
        run(&mut command, Duration::from_secs(30)).unwrap();
        assert!(
            io::read_regular(&scratch.0.join("save.dll"), 1024 * 1024)
                .unwrap()
                .starts_with(b"MZ")
        );
    }
}
