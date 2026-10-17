//! MSI transport regression only. The fixture has metadata and no action or
//! installation sequence; the only package opener ignores machine state.
use super::*;

const MSI_FIXTURE_API: &str = r#"
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class LanGameMsiFixture {
    [DllImport("msi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] public static extern uint MsiOpenDatabaseW(string path, IntPtr mode, out uint database);
    [DllImport("msi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] private static extern uint MsiDatabaseOpenViewW(uint database, string sql, out uint view);
    [DllImport("msi.dll", ExactSpelling=true)] private static extern uint MsiViewExecute(uint view, uint record);
    [DllImport("msi.dll", ExactSpelling=true)] private static extern uint MsiDatabaseCommit(uint database);
    [DllImport("msi.dll", ExactSpelling=true)] public static extern uint MsiCloseHandle(uint handle);
    [DllImport("msi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] private static extern uint MsiGetSummaryInformationW(uint database, string path, uint count, out uint summary);
    [DllImport("msi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] private static extern uint MsiSummaryInfoSetPropertyW(uint summary, uint property, uint type, int integer, IntPtr time, string text);
    [DllImport("msi.dll", ExactSpelling=true)] private static extern uint MsiSummaryInfoPersist(uint summary);
    [DllImport("msi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] public static extern uint MsiVerifyPackageW(string path);
    [DllImport("msi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] public static extern uint MsiOpenPackageExW(string path, uint flags, out uint product);
    [DllImport("msi.dll", ExactSpelling=true)] public static extern uint MsiSetInternalUI(uint level, IntPtr window);
    [DllImport("shell32.dll", CharSet=CharSet.Unicode, SetLastError=true)] private static extern IntPtr CommandLineToArgvW(string line, out int count);
    [DllImport("kernel32.dll")] private static extern IntPtr LocalFree(IntPtr memory);
    private static void Check(uint result) { if (result != 0) throw new Exception("MSI fixture API failed: " + result); }
    private static void Sql(uint database, string sql) {
        uint view; Check(MsiDatabaseOpenViewW(database, sql, out view));
        try { Check(MsiViewExecute(view, 0)); } finally { MsiCloseHandle(view); }
    }
    public static void Create(string path) {
        uint database; Check(MsiOpenDatabaseW(path, new IntPtr(3), out database));
        try {
            Sql(database, "CREATE TABLE `Property` (`Property` CHAR(72) NOT NULL, `Value` CHAR(0) LOCALIZABLE PRIMARY KEY `Property`)");
            Sql(database, "INSERT INTO `Property` (`Property`,`Value`) VALUES ('ProductCode','{" + Guid.NewGuid().ToString().ToUpperInvariant() + "}')");
            Sql(database, "INSERT INTO `Property` (`Property`,`Value`) VALUES ('ProductVersion','1.0.0')");
            Sql(database, "INSERT INTO `Property` (`Property`,`Value`) VALUES ('ProductName','LanGame readonly transport fixture')");
            Sql(database, "INSERT INTO `Property` (`Property`,`Value`) VALUES ('ProductLanguage','1033')");
            uint summary; Check(MsiGetSummaryInformationW(database, null, 20, out summary));
            try {
                Check(MsiSummaryInfoSetPropertyW(summary, 7, 30, 0, IntPtr.Zero, "Intel;1033"));
                Check(MsiSummaryInfoSetPropertyW(summary, 9, 30, 0, IntPtr.Zero, "{" + Guid.NewGuid().ToString().ToUpperInvariant() + "}"));
                Check(MsiSummaryInfoSetPropertyW(summary, 14, 3, 200, IntPtr.Zero, null));
                Check(MsiSummaryInfoPersist(summary));
            } finally { MsiCloseHandle(summary); }
            Check(MsiDatabaseCommit(database));
        } finally { MsiCloseHandle(database); }
    }
    public static string[] Arguments(string line) {
        int count; IntPtr memory = CommandLineToArgvW(line, out count);
        if (memory == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        try {
            string[] result = new string[count];
            for (int i = 0; i < count; ++i) result[i] = Marshal.PtrToStringUni(Marshal.ReadIntPtr(memory, i * IntPtr.Size));
            return result;
        } finally { LocalFree(memory); }
    }
    public static uint OpenReadOnly(string path) {
        uint product; uint result = MsiOpenPackageExW(path, 1, out product);
        if (product != 0) MsiCloseHandle(product);
        return result;
    }
}
'@
"#;

#[test]
fn native_msi_parameters_open_mixed_unicode_paths_under_the_full_source_lease() {
    let fixture = RuntimeFixture::new();
    let root = fixture
        .0
        .join("服务端 O'Brien")
        .join("runtimes/windows-packages");
    fs::create_dir_all(&root).unwrap();
    let package = root.join("泰拉瑞亚 O'Brien.msi");
    let setup = process::msi_parameters_script(&package, &root);
    let script = format!(
        r#"$ErrorActionPreference='Stop'
{api}
{lease}
$raw={package}
$native=[IO.Path]::GetFullPath($raw)
$oldUI=[LanGameMsiFixture]::MsiSetInternalUI(2,[IntPtr]::Zero)
$sourceLease=$null
try {{
    [LanGameMsiFixture]::Create($native)
    if ([LanGameMsiFixture]::MsiVerifyPackageW($raw) -ne 0) {{ throw 'Fixture must be a valid MSI before testing transport.' }}
    if ([LanGameMsiFixture]::OpenReadOnly($native) -ne 0) {{ throw 'Native fixture baseline could not be opened.' }}
    $sourceLease=[LanGameRuntimeFileLease]::Acquire($raw)
    foreach ($readonly in @($false,$true)) {{
        if ($readonly) {{ [IO.File]::SetAttributes($native,[IO.FileAttributes]::ReadOnly) }}
        # This is the previous transport: paths containing the exact embedded
        # runtimes/windows-packages separator remain valid filesystem paths,
        # but the Windows Installer package opener returns 1619.
        if ([LanGameMsiFixture]::OpenReadOnly($raw) -ne 1619) {{ throw 'Mixed separator transport did not reproduce MSI 1619.' }}
        {setup}
        $argv=[LanGameMsiFixture]::Arguments('msiexec.exe '+$arguments)
        if ($argv.Count -ne 7 -or $argv[1] -ne '/i' -or $argv[2] -ne $native -or
            $argv[3] -ne '/quiet' -or $argv[4] -ne '/norestart' -or $argv[5] -ne '/L*V') {{
            throw 'Generated MSI command line changed quoting or install/log switches.'
        }}
        if ($argv[2].Contains('/') -or $argv[6].Contains('/') -or ![IO.Path]::IsPathRooted($argv[6])) {{
            throw 'MSI package and log paths must be absolute native Windows paths.'
        }}
        $allowed=[IO.Path]::GetFullPath({root}).TrimEnd('\')+'\'
        if (!$argv[6].StartsWith($allowed,[StringComparison]::OrdinalIgnoreCase)) {{ throw 'MSI log escaped its cache root.' }}
        if ([IO.File]::Exists($argv[6])) {{ throw 'Parameter preparation must not pretend to have produced an MSI log.' }}
        $sourceLease.Validate()
        if ([LanGameMsiFixture]::OpenReadOnly($argv[2]) -ne 0) {{ throw 'Fixed native MSI path failed under the full source and parent lease.' }}
    }}
}} finally {{
    if ($null -ne $sourceLease) {{ $sourceLease.Dispose() }}
    if ([IO.File]::Exists($native)) {{ [IO.File]::SetAttributes($native,[IO.FileAttributes]::Normal) }}
    $null=[LanGameMsiFixture]::MsiSetInternalUI($oldUI,[IntPtr]::Zero)
}}
"#,
        api = MSI_FIXTURE_API,
        lease = process::FILE_LEASE_FUNCTION,
        package = ps_literal(&package),
        root = ps_literal(&root)
    );
    let output = capture_fixture_script(&script);
    assert!(
        output.status.success(),
        "MSI transport regression failed: {}",
        output_excerpt(&output.stdout, &output.stderr)
    );
}

#[test]
fn msi_diagnostics_only_claim_a_log_when_the_file_exists() {
    let fixture = RuntimeFixture::new();
    let log = fixture.0.join("MSI 日志 O'Brien.log");
    let path = log.to_str().unwrap();
    assert!(process::msi_log_detail(Some(path)).contains("日志未生成"));
    fs::write(&log, b"owned diagnostic fixture").unwrap();
    assert!(process::msi_log_detail(Some(path)).contains("MSI 日志："));
    assert!(!process::msi_log_detail(Some(path)).contains("日志未生成"));
    assert_eq!(process::msi_log_detail(None), "");
}
