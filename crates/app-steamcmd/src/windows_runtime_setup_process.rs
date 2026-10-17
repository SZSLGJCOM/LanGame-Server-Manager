use super::*;
use app_runtime::{WindowsRuntimeArchitecture as Architecture, WindowsRuntimeFamily as Family};

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeProbe {
    pub ready: bool,
    pub version: Option<String>,
}

#[cfg(windows)]
fn powershell(architecture: Architecture) -> Result<PathBuf, SteamCmdError> {
    let system = app_runtime::windows_system_directory()
        .map_err(|error| runtime_error(format!("无法定位 Windows 系统目录：{error}")))?;
    let directory = if architecture == Architecture::X86 && cfg!(target_pointer_width = "64") {
        system
            .parent()
            .ok_or_else(|| runtime_error("Windows 系统目录缺少父目录。"))?
            .join("SysWOW64")
    } else {
        system
    };
    let executable = directory.join("WindowsPowerShell/v1.0/powershell.exe");
    if !executable.is_file() {
        return Err(runtime_error(
            "Windows PowerShell 5.1 不可用，无法验证运行库。",
        ));
    }
    Ok(executable)
}

#[cfg(windows)]
async fn capture(
    script: &str,
    architecture: Architecture,
    deadline: InstallDeadline,
) -> Result<std::process::Output, SteamCmdError> {
    let executable = powershell(architecture)?;
    let mut command = Command::new(&executable);
    apply_no_window(&mut command);
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    command.current_dir(
        executable
            .parent()
            .ok_or_else(|| runtime_error("PowerShell 目录无效。"))?,
    );
    let output = run_command_capture(command, deadline).await?;
    if !output.status.success() {
        return Err(runtime_error(format!(
            "Windows 运行库检查失败：{}",
            output_excerpt(&output.stdout, &output.stderr)
        )));
    }
    Ok(output)
}

#[cfg(windows)]
pub(super) async fn probe(
    requirement: Option<&WindowsRuntimeRequirement>,
    deadline: InstallDeadline,
) -> Result<RuntimeProbe, SteamCmdError> {
    let architecture = requirement.map_or(Architecture::X86, |value| value.architecture);
    let script = probe_script(requirement);
    let output = capture(&script, architecture, deadline).await?;
    serde_json::from_slice(&output.stdout)
        .map_err(|error| runtime_error(format!("Windows 运行库检查返回了无效结果：{error}")))
}

#[cfg(windows)]
pub(super) async fn probe_all(
    requirements: &[WindowsRuntimeRequirement],
    deadline: InstallDeadline,
) -> Result<RuntimeProbe, SteamCmdError> {
    if requirements.is_empty() {
        return probe(None, deadline).await;
    }
    let mut combined = RuntimeProbe {
        ready: true,
        version: None,
    };
    for requirement in requirements {
        let result = probe(Some(requirement), deadline).await?;
        combined.ready &= result.ready;
        // Plans share a package and architecture, except DirectX (which has no
        // version update stream). Every actual importing directory is probed.
        if let Some(version) = result.version {
            combined.version = Some(version);
        }
    }
    Ok(combined)
}

const SCRIPT_HEADER: &str = "$ErrorActionPreference='Stop'\n[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)\n";
pub(super) const SIGNATURE_FUNCTION: &str = r#"
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Security.Cryptography.X509Certificates;
using System.Text;
public static class LanGameRuntimeSigner {
    [DllImport("crypt32.dll", EntryPoint="CertGetNameStringW", CharSet=CharSet.Unicode, ExactSpelling=true)]
    private static extern uint CertGetNameString(IntPtr certificate, uint type, uint flags, [MarshalAs(UnmanagedType.LPStr)] string oid, StringBuilder name, uint capacity);
    public static string Organization(X509Certificate2 certificate) {
        if (certificate == null) return String.Empty;
        try {
            // CERT_NAME_ATTR_TYPE reads the real X.509 Organization attribute;
            // the formatted Subject can contain an apparent O= inside a CN.
            uint length = CertGetNameString(certificate.Handle, 3, 0, "2.5.4.10", null, 0);
            if (length <= 1) return String.Empty;
            if (length > 32768) throw new CryptographicException("Certificate organization is too long.");
            StringBuilder organization = new StringBuilder((int)length);
            if (CertGetNameString(certificate.Handle, 3, 0, "2.5.4.10", organization, length) != length)
                throw new CryptographicException("Could not read the certificate organization.");
            return organization.ToString();
        } finally { GC.KeepAlive(certificate); }
    }
}
'@
function Assert-MicrosoftSignature([string]$Path) {
    $signature=Get-AuthenticodeSignature -LiteralPath $Path -ErrorAction Stop
    if ($signature.Status -ne 'Valid' -or $null -eq $signature.SignerCertificate -or
        ![String]::Equals([LanGameRuntimeSigner]::Organization($signature.SignerCertificate), 'Microsoft Corporation', [StringComparison]::OrdinalIgnoreCase)) {
        throw '运行库文件的 Microsoft Authenticode 签名无效；已拒绝执行。'
    }
}
"#;

// Native handles bind every path component and the file's identity before
// Authenticode reads by name. Read sharing permits the Windows installer to
// open the same bytes; write/delete sharing is intentionally withheld.
pub(super) const FILE_LEASE_FUNCTION: &str = r#"
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Collections.Generic;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
public sealed class LanGameRuntimeFileLease : IDisposable {
    [StructLayout(LayoutKind.Sequential)] private struct Information {
        public uint Attributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
        public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    private static extern SafeFileHandle CreateFile(string path, uint access, uint sharing, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern bool GetFileInformationByHandle(SafeFileHandle file, out Information info);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    private static extern uint GetFinalPathNameByHandle(SafeFileHandle file, StringBuilder path, uint capacity, uint flags);
    private readonly List<SafeFileHandle> directories = new List<SafeFileHandle>();
    private FileStream source;
    private Information identity;
    private string path;
    private static string Normalize(string value) {
        if (value.StartsWith(@"\\?\UNC\", StringComparison.OrdinalIgnoreCase)) value = @"\\" + value.Substring(8);
        else if (value.StartsWith(@"\\?\", StringComparison.Ordinal)) value = value.Substring(4);
        return Path.GetFullPath(value).TrimEnd(Path.DirectorySeparatorChar);
    }
    private static Information Inspect(SafeFileHandle handle, string expected) {
        Information info;
        if (!GetFileInformationByHandle(handle, out info)) throw new Win32Exception(Marshal.GetLastWin32Error());
        if ((info.Attributes & 0x400) != 0) throw new IOException("Runtime paths must not contain reparse points.");
        StringBuilder actual = new StringBuilder(32768);
        uint length = GetFinalPathNameByHandle(handle, actual, (uint)actual.Capacity, 0);
        if (length == 0 || length >= actual.Capacity) throw new Win32Exception(Marshal.GetLastWin32Error());
        if (!String.Equals(Normalize(actual.ToString()), Normalize(expected), StringComparison.OrdinalIgnoreCase))
            throw new IOException("Runtime path resolved through an unexpected alias.");
        return info;
    }
    public static LanGameRuntimeFileLease Acquire(string target) {
        LanGameRuntimeFileLease lease = new LanGameRuntimeFileLease();
        try {
            if (!Path.IsPathRooted(target)) throw new IOException("Runtime path must be absolute.");
            lease.path = Path.GetFullPath(target);
            Stack<string> parents = new Stack<string>();
            for (string current = Path.GetDirectoryName(lease.path); !String.IsNullOrEmpty(current); current = Path.GetDirectoryName(current)) parents.Push(current);
            while (parents.Count > 0) {
                string current = parents.Pop();
                // Attribute-only handles do not participate in data sharing.
                // FILE_LIST_DIRECTORY registers a read open so FILE_SHARE_READ
                // rejects directory data writers, deletion and renaming.
                SafeFileHandle directory = CreateFile(current, 0x81, 1, IntPtr.Zero, 3, 0x02200000, IntPtr.Zero);
                if (directory.IsInvalid) { int error = Marshal.GetLastWin32Error(); directory.Dispose(); throw new Win32Exception(error); }
                lease.directories.Add(directory);
                Inspect(directory, current);
            }
            lease.source = File.Open(lease.path, FileMode.Open, FileAccess.Read, FileShare.Read);
            lease.identity = Inspect(lease.source.SafeFileHandle, lease.path);
            return lease;
        } catch { lease.Dispose(); throw; }
    }
    public void Validate() {
        using (FileStream current = File.Open(path, FileMode.Open, FileAccess.Read, FileShare.Read)) {
            Information observed = Inspect(current.SafeFileHandle, path);
            if (identity.Volume != observed.Volume || identity.IndexHigh != observed.IndexHigh || identity.IndexLow != observed.IndexLow)
                throw new IOException("Runtime source identity changed.");
        }
    }
    public void Dispose() {
        if (source != null) { source.Dispose(); source = null; }
        for (int i = directories.Count - 1; i >= 0; --i) directories[i].Dispose();
        directories.Clear();
    }
}
'@
"#;

pub(super) const NATIVE_LOADER_FUNCTION: &str = r#"
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class LanGameRuntimeLoader {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr LoadLibraryEx(string name, IntPtr file, uint flags);
    [DllImport("kernel32.dll")] public static extern bool FreeLibrary(IntPtr module);
}
'@
"#;

pub(super) fn probe_script(requirement: Option<&WindowsRuntimeRequirement>) -> String {
    let Some(requirement) = requirement else {
        // XNA 4 is a .NET Framework assembly. Probe it in a 32-bit Framework
        // PowerShell process, not CoreCLR / .NET (which has a different loader).
        return format!(
            r#"{SCRIPT_HEADER}$ready=$true
foreach ($assembly in @('Microsoft.Xna.Framework','Microsoft.Xna.Framework.Game','Microsoft.Xna.Framework.Graphics','Microsoft.Xna.Framework.Xact')) {{
    try {{ $null=[Reflection.Assembly]::Load($assembly+', Version=4.0.0.0, Culture=neutral, PublicKeyToken=842cf8be1de50553') }}
    catch [System.IO.FileNotFoundException] {{ $ready=$false }}
    catch [System.IO.FileLoadException] {{ $ready=$false }}
    catch [System.BadImageFormatException] {{ $ready=$false }}
}}
@{{ready=$ready;version=$null}}|ConvertTo-Json -Compress
"#
        );
    };
    let libraries = requirement
        .imported_dlls
        .iter()
        .map(|name| quote(name))
        .collect::<Vec<_>>()
        .join(",");
    let directories = requirement
        .search_directories
        .iter()
        .map(|path| ps_literal(path))
        .collect::<Vec<_>>()
        .join(",");
    let registry = if requirement.family == Family::VisualCpp2015 {
        let architecture = if requirement.architecture == Architecture::X86 {
            "x86"
        } else {
            "x64"
        };
        format!(
            r#"
foreach ($view in @([Microsoft.Win32.RegistryView]::Registry64,[Microsoft.Win32.RegistryView]::Registry32)) {{
    $base=$null
    $key=$null
    try {{
        $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine,$view)
        $key=$base.OpenSubKey('SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\{architecture}')
        if ($null -ne $key -and $key.GetValue('Installed') -eq 1) {{
            $found=[string]$key.GetValue('Version')
            if ($found -match '^v?\d+(\.\d+){{1,3}}$' -and
                ($null -eq $version -or [Version]$found.TrimStart('v') -gt [Version]$version.TrimStart('v'))) {{ $version=$found }}
        }}
    }} finally {{
        if ($null -ne $key) {{ $key.Dispose() }}
        if ($null -ne $base) {{ $base.Dispose() }}
    }}
}}
"#
        )
    } else {
        String::new()
    };
    format!(
        r#"{SCRIPT_HEADER}{SIGNATURE_FUNCTION}{FILE_LEASE_FUNCTION}{NATIVE_LOADER_FUNCTION}
$ready=$true
$version=$null
$directories=@({directories})
$leases=New-Object 'System.Collections.Generic.List[IDisposable]'
$loaded=New-Object 'System.Collections.Generic.List[IntPtr]'
$pending=New-Object 'System.Collections.Generic.List[object]'
try {{
foreach ($library in @({libraries})) {{
    $candidate=$null
    foreach ($directory in $directories) {{
        $local=Join-Path $directory $library
        if ([IO.File]::Exists($local)) {{ $candidate=$local; break }}
    }}
    if ($null -eq $candidate) {{
        $candidate=Join-Path ([Environment]::SystemDirectory) $library
    }} else {{
        # An application-local runtime wins Windows DLL resolution. Verify it
        # before loading; loading an arbitrary game DLL would execute its DllMain.
        try {{
            $lease=[LanGameRuntimeFileLease]::Acquire($candidate)
            $leases.Add($lease)
            Assert-MicrosoftSignature $candidate
            $lease.Validate()
        }} catch {{ $ready=$false; continue }}
    }}
    $priority=1
    if ($library -match '^(ucrtbase|vcruntime\d+(_\d+)?|msvcr\d+)\.dll$') {{ $priority=0 }}
    $pending.Add([PSCustomObject]@{{Path=$candidate;Priority=$priority}})
}}
if ($ready) {{
    # Preload verified base CRTs and retain their handles. Loaded-module lookup
    # can then satisfy dependent app-local runtimes without searching arbitrary
    # neighboring DLLs. Retry only while a pass makes measurable progress.
    $ordered=@($pending | Sort-Object Priority)
    $pending.Clear()
    foreach ($entry in $ordered) {{ $pending.Add($entry) }}
    $budget=[Math]::Min(128,$pending.Count)
    for ($pass=0; $pass -lt $budget -and $pending.Count -gt 0; $pass++) {{
        $advanced=$false
        foreach ($entry in $pending.ToArray()) {{
            $candidate=$entry.Path
            $handle=[LanGameRuntimeLoader]::LoadLibraryEx($candidate,[IntPtr]::Zero,0x800)
            if ($handle -ne [IntPtr]::Zero) {{
                $loaded.Add($handle)
                $null=$pending.Remove($entry)
                $advanced=$true
            }}
        }}
        if (!$advanced) {{ break }}
    }}
    $ready=($pending.Count -eq 0)
}}
}} finally {{
    for ($index=$loaded.Count-1; $index -ge 0; $index--) {{ $null=[LanGameRuntimeLoader]::FreeLibrary($loaded[$index]) }}
    for ($index=$leases.Count-1; $index -ge 0; $index--) {{ $leases[$index].Dispose() }}
}}
{registry}
@{{ready=$ready;version=$version}}|ConvertTo-Json -Compress
"#
    )
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(windows)]
pub(super) async fn verify_package(
    path: &Path,
    package: RuntimePackage,
    deadline: InstallDeadline,
) -> Result<(), SteamCmdError> {
    let script = format!(
        "{SCRIPT_HEADER}{SIGNATURE_FUNCTION}\nAssert-MicrosoftSignature {}\n{}",
        ps_literal(path),
        digest_check(path, package)
    );
    capture(&script, Architecture::X64, deadline).await?;
    Ok(())
}

#[cfg(windows)]
pub(super) async fn package_version(
    path: &Path,
    deadline: InstallDeadline,
) -> Result<String, SteamCmdError> {
    let script = format!(
        "{SCRIPT_HEADER}[Diagnostics.FileVersionInfo]::GetVersionInfo({}).ProductVersion\n",
        ps_literal(path)
    );
    let output = capture(&script, Architecture::X64, deadline).await?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(windows)]
pub(super) async fn boot_identity(deadline: InstallDeadline) -> Result<String, SteamCmdError> {
    let script = format!(
        "{SCRIPT_HEADER}(Get-CimInstance Win32_OperatingSystem -ErrorAction Stop).LastBootUpTime.ToUniversalTime().Ticks.ToString()\n"
    );
    let output = capture(&script, Architecture::X64, deadline).await?;
    let identity = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if identity.is_empty() || !identity.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(runtime_error(
            "无法确认 Windows 启动标识，已阻止系统运行库安装。",
        ));
    }
    Ok(identity)
}

#[cfg(windows)]
pub(super) async fn check_reboot_marker(
    root: &Path,
    deadline: InstallDeadline,
) -> Result<(), SteamCmdError> {
    use std::io::Read;
    for name in [REBOOT_MARKER, INSTALLING_MARKER] {
        let marker = root.join(name);
        validate_cache_path(&marker)?;
        let mut file = match fs::File::open(&marker) {
            Ok(file) => file.take(4097),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(runtime_error(format!("无法读取运行库安装状态：{error}"))),
        };
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|error| runtime_error(format!("无法读取运行库安装状态：{error}")))?;
        if text.len() > 4096 {
            return Err(runtime_error("运行库安装状态文件过大。"));
        }
        let saved: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| runtime_error(format!("运行库安装状态损坏：{error}")))?;
        let saved_boot = saved
            .get("boot_identity")
            .and_then(serde_json::Value::as_str)
            .filter(|identity| {
                !identity.is_empty() && identity.bytes().all(|byte| byte.is_ascii_digit())
            })
            .ok_or_else(|| runtime_error("运行库安装状态缺少有效的启动标识。"))?;
        if saved_boot == boot_identity(deadline).await? {
            let detail = if name == REBOOT_MARKER {
                "Windows 运行库安装已完成，但需要先重启电脑，再启动实例。"
            } else {
                "上次 Windows 运行库安装的结果尚未确认；已阻止重复安装。请检查系统安装器，必要时重启电脑后重试。"
            };
            return Err(runtime_error(detail));
        }
        fs::remove_file(&marker)
            .map_err(|error| runtime_error(format!("无法更新运行库安装状态：{error}")))?;
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn install(
    package: RuntimePackage,
    path: &Path,
    root: &Path,
    boot: &str,
) -> Result<(), SteamCmdError> {
    validate_cache_path(path)?;
    let executable = powershell(Architecture::X64)?;
    let script = installer_script(package, path, root);
    let pending = root.join(INSTALLING_MARKER);
    publish_install_state(root, INSTALLING_MARKER, boot, package.id)?;
    let mut command = std::process::Command::new(&executable);
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW).args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &script,
    ]);
    command.current_dir(
        executable
            .parent()
            .ok_or_else(|| runtime_error("PowerShell 目录无效。"))?,
    );
    // Intentionally independent of the cancellable Job helpers. This worker
    // owns the install lease until the elevated child has really terminated.
    let output = command
        .output()
        .map_err(|error| runtime_error(format!("无法启动受信任的运行库安装器：{error}")))?;
    #[derive(Deserialize)]
    struct ResultCode {
        exit_code: Option<u32>,
        started: bool,
        detail: Option<String>,
        log_path: Option<String>,
    }
    let result: ResultCode = serde_json::from_slice(&output.stdout)
        .map_err(|error| runtime_error(format!("运行库安装失败、UAC 已取消或安装状态未知，已阻止启动；请检查 Windows 安装器：{error}；{}", output_excerpt(&output.stdout, &output.stderr))))?;
    let log_detail = msi_log_detail(result.log_path.as_deref());
    let Some(code) = result.exit_code.filter(|_| result.started) else {
        if !result.started {
            fs::remove_file(&pending).map_err(|error| {
                runtime_error(format!("无法清理未执行的运行库安装事务：{error}"))
            })?;
            return Err(runtime_error(format!(
                "运行库安装尚未执行或 UAC 已取消：{}{}",
                result.detail.as_deref().unwrap_or("系统安装器未返回原因"),
                log_detail
            )));
        }
        return Err(runtime_error(
            "运行库安装状态未知；已阻止重复安装，请检查 Windows 安装器，必要时重启电脑。",
        ));
    };
    // Publish a restart requirement before resolving the pending transaction.
    // A crash between the writes must retain at least one blocking marker.
    let outcome = match classify_installer_exit(code) {
        InstallerExit::Success if output.status.success() => Ok(()),
        InstallerExit::Success => Err(runtime_error(format!(
            "运行库安装器已退出，但清理检查失败：{}{}",
            output_excerpt(&output.stdout, &output.stderr),
            log_detail
        ))),
        InstallerExit::RebootRequired => {
            publish_install_state(root, REBOOT_MARKER, boot, package.id)?;
            Err(runtime_error(format!(
                "{} 安装完成，但 Windows 要求重启电脑（{}）；已阻止实例启动。{}",
                package.name, code, log_detail
            )))
        }
        InstallerExit::Busy => Err(runtime_error(format!(
            "另一个 Windows 安装器正在运行（1618）；已阻止启动，请等待该安装完成后重试。{}",
            log_detail
        ))),
        InstallerExit::Failed if code == 1619 => Err(runtime_error(format!(
            "{} 安装失败（1619：Windows Installer 无法打开安装包）；已阻止实例启动。{}",
            package.name, log_detail
        ))),
        InstallerExit::Failed => Err(runtime_error(format!(
            "{} 安装失败（{}）；已阻止实例启动。{}",
            package.name, code, log_detail
        ))),
    };
    // Unknown output never reaches this point, so killing the waiter cannot
    // enable a blind retry while an elevated child may still be running.
    fs::remove_file(&pending)
        .map_err(|error| runtime_error(format!("无法完成运行库安装事务：{error}")))?;
    outcome
}

pub(super) fn msi_log_detail(log_path: Option<&str>) -> String {
    match log_path {
        Some(path) => match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => format!(" MSI 日志：{path}"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                format!(" MSI 日志未生成（计划路径：{path}）。")
            }
            Ok(_) => format!(" MSI 日志路径不是普通文件：{path}"),
            Err(error) => format!(" 无法确认 MSI 日志（路径：{path}）：{error}"),
        },
        None => String::new(),
    }
}

pub(super) fn publish_install_state(
    root: &Path,
    name: &str,
    boot: &str,
    package_id: &str,
) -> Result<(), SteamCmdError> {
    let marker = root.join(name);
    validate_cache_path(&marker)?;
    let state = serde_json::json!({"boot_identity":boot,"package":package_id}).to_string();
    write_minecraft_server_file_atomically(&marker, state.as_bytes()).map_err(|error| {
        runtime_error(format!(
            "无法原子记录运行库安装状态；未确认的安装不会继续：{error}"
        ))
    })
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum InstallerExit {
    Success,
    RebootRequired,
    Busy,
    Failed,
}

pub(super) fn classify_installer_exit(code: u32) -> InstallerExit {
    match code {
        0 => InstallerExit::Success,
        3010 | 1641 => InstallerExit::RebootRequired,
        1618 => InstallerExit::Busy,
        _ => InstallerExit::Failed,
    }
}

fn digest_check(path: &Path, package: RuntimePackage) -> String {
    package.sha256.map(|expected| format!("if ((Get-FileHash -LiteralPath {} -Algorithm SHA256).Hash -ne {}) {{ throw '运行库安装包 SHA-256 校验失败。' }}\n", ps_literal(path), quote(expected))).unwrap_or_default()
}

pub(super) fn installer_script(package: RuntimePackage, path: &Path, root: &Path) -> String {
    use catalog::InstallerKind;
    let setup = match package.kind {
        InstallerKind::XnaMsi => msi_parameters_script(path, root),
        InstallerKind::VisualCpp => {
            let arguments = if package.id.starts_with("vc-2010-") {
                "/q /norestart"
            } else {
                "/install /quiet /norestart"
            };
            format!(
                "$file={}\n$arguments={}\n",
                ps_literal(path),
                quote(arguments)
            )
        }
        InstallerKind::DirectX => format!(
            r#"
$stage=Join-Path {root} ('directx-extract-'+[Guid]::NewGuid().ToString('N'))
$null=[IO.Directory]::CreateDirectory($stage)
$extract=Start-Process -FilePath {path} -ArgumentList ('/Q /T:"'+$stage+'"') -PassThru -WindowStyle Hidden
$extract.WaitForExit()
if ($extract.ExitCode -ne 0) {{ throw ('DirectX 解压失败：'+$extract.ExitCode) }}
$file=Join-Path $stage 'DXSETUP.exe'
$payload=@(Get-ChildItem -LiteralPath $stage -Force)
if ($payload.Count -gt 512) {{ throw 'DirectX 安装包文件数超出上限。' }}
foreach ($entry in $payload) {{
    if ($entry.PSIsContainer) {{ throw 'DirectX 安装包包含非预期子目录。' }}
    $lease=[LanGameRuntimeFileLease]::Acquire($entry.FullName)
    $payloadLeases.Add($lease)
    if ($entry.Extension -in @('.exe','.dll')) {{ Assert-MicrosoftSignature $entry.FullName }}
    $lease.Validate()
}}
Assert-MicrosoftSignature $file
$arguments='/silent'
"#,
            root = ps_literal(root),
            path = ps_literal(path)
        ),
    };
    format!(
        r#"{SCRIPT_HEADER}{SIGNATURE_FUNCTION}{FILE_LEASE_FUNCTION}
$mutex=$null
$acquired=$false
$installer=$null
$extract=$null
$stage=$null
$failure=$null
$logPath=$null
$packageLease=$null
$payloadLeases=New-Object 'System.Collections.Generic.List[IDisposable]'
try {{
    # A machine-wide mutex prevents another LGSM installer from running in
    # parallel. An inaccessible mutex fails closed; MSI also returns 1618 for
    # unrelated installers that do not participate in this coordination.
    $mutex=New-Object System.Threading.Mutex($false,'Global\LanGame.WindowsRuntimePrerequisites')
    try {{ $acquired=$mutex.WaitOne([TimeSpan]::FromMinutes(10)) }} catch [System.Threading.AbandonedMutexException] {{ $acquired=$true }}
    if (!$acquired) {{ throw '等待其他运行库安装器超时；未启动新的安装。' }}
    $packageLease=[LanGameRuntimeFileLease]::Acquire({path})
    Assert-MicrosoftSignature {path}
    {digest}
    {setup}
    $packageLease.Validate()
    foreach ($lease in $payloadLeases) {{ $lease.Validate() }}
    $installer=Start-Process -FilePath $file -ArgumentList $arguments -Verb RunAs -PassThru -WindowStyle Hidden
    $installer.WaitForExit()
}} catch {{
    $failure=$_.Exception.Message
}} finally {{
    # A failed async waiter, a cancellation, or a script error cannot release
    # the mutex while an installer or extractor still performs system changes.
    if ($null -ne $extract -and !$extract.HasExited) {{ $extract.WaitForExit() }}
    if ($null -ne $installer -and !$installer.HasExited) {{ $installer.WaitForExit() }}
    try {{
        for ($index=$payloadLeases.Count-1; $index -ge 0; $index--) {{ $payloadLeases[$index].Dispose() }}
        if ($null -ne $packageLease) {{ $packageLease.Dispose() }}
        if ($null -ne $stage -and [IO.Directory]::Exists($stage)) {{
            $fullStage=[IO.Path]::GetFullPath($stage)
            $allowedRoot=[IO.Path]::GetFullPath({root}).TrimEnd('\')+'\'
            if (!$fullStage.StartsWith($allowedRoot,[StringComparison]::OrdinalIgnoreCase) -or
                ((Get-Item -LiteralPath $stage -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {{
                throw 'DirectX 临时目录归属校验失败；保留该目录。'
            }}
            Remove-Item -LiteralPath $stage -Recurse -Force
        }}
    }} finally {{
        if ($acquired) {{ $mutex.ReleaseMutex() }}
        if ($null -ne $mutex) {{ $mutex.Dispose() }}
    }}
}}
$code=$null
if ($null -ne $installer) {{ $code=$installer.ExitCode }}
@{{exit_code=$code;started=($null -ne $installer);detail=$failure;log_path=$logPath}}|ConvertTo-Json -Compress
"#,
        path = ps_literal(path),
        root = ps_literal(root),
        digest = digest_check(path, package)
    )
}

pub(super) fn msi_parameters_script(path: &Path, root: &Path) -> String {
    // Windows filesystem APIs accept mixed separators, but the MSI package
    // engine can reject the same existing path with ERROR_INSTALL_PACKAGE_OPEN_FAILED.
    format!(
        r#"
$file=Join-Path ([Environment]::SystemDirectory) 'msiexec.exe'
$packagePath=[IO.Path]::GetFullPath({path})
$logPath=[IO.Path]::GetFullPath((Join-Path {root} ('xna-4-refresh-msi-'+[Guid]::NewGuid().ToString('N')+'.log')))
$arguments='/i "'+$packagePath+'" /quiet /norestart /L*V "'+$logPath+'"'
"#,
        path = ps_literal(path),
        root = ps_literal(root)
    )
}
