use super::{capture_fixture_script, output_excerpt, process};

// Certificates and keys exist only in memory. Exporting and reloading public DER
// ensures the native reader receives actual X.509 attributes, not a Subject mock.
const DER_FIXTURE: &str = r#"
function New-DerFixture([string]$Subject) {
    $key=[Security.Cryptography.RSACng]::new(2048)
    try {
        $request=[Security.Cryptography.X509Certificates.CertificateRequest]::new(
            [Security.Cryptography.X509Certificates.X500DistinguishedName]::new($Subject),
            $key, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
        $source=$request.CreateSelfSigned([DateTimeOffset]::UtcNow.AddMinutes(-1), [DateTimeOffset]::UtcNow.AddMinutes(5))
        try {
            $der=$source.Export([Security.Cryptography.X509Certificates.X509ContentType]::Cert)
            return [Security.Cryptography.X509Certificates.X509Certificate2]::new($der)
        } finally { $source.Dispose() }
    } finally { $key.Dispose() }
}
function Get-AuthenticodeSignature {
    [CmdletBinding()] param([string]$LiteralPath)
    return $script:FixtureSignature
}
function Test-SignatureAcceptance($Certificate, [string]$Status) {
    # Only the external Authenticode result is replaced. Both the DER attribute
    # reader and the production authorization decision execute unchanged.
    $script:FixtureSignature=[pscustomobject]@{Status=$Status;SignerCertificate=$Certificate}
    try { Assert-MicrosoftSignature 'in-memory boundary fixture; never executed'; return $true }
    catch { return $false }
}
"#;

fn fixture_result(body: &str) -> serde_json::Value {
    let script = format!(
        "$ErrorActionPreference='Stop'\n{}\n{DER_FIXTURE}\n{body}",
        process::SIGNATURE_FUNCTION
    );
    let output = capture_fixture_script(&script);
    assert!(
        output.status.success(),
        "Certificate fixture failed: {}",
        output_excerpt(&output.stdout, &output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("Certificate fixture must return JSON")
}

#[test]
fn signer_organization_uses_der_attributes_and_rejects_cn_field_spoofing() {
    let result = fixture_result(
        r#"
$subjects=@('CN="Example, O=Microsoft Corporation, suffix", O=Example', 'CN=Runtime fixture, O=Microsoft Corporation', 'CN=Runtime fixture')
$rows=@(foreach ($subject in $subjects) {
    $certificate=New-DerFixture $subject
    try {
        [pscustomobject]@{
            subject=$certificate.Subject
            old_regex_matches=($certificate.Subject -match '(^|,\s*)O=Microsoft Corporation(,|$)')
            organization=[LanGameRuntimeSigner]::Organization($certificate)
            accepted=(Test-SignatureAcceptance $certificate 'Valid')
        }
    } finally { $certificate.Dispose() }
})
ConvertTo-Json -InputObject $rows -Compress
"#,
    );
    assert_eq!(result.as_array().unwrap().len(), 3);
    assert!(
        result[0]["subject"]
            .as_str()
            .unwrap()
            .contains("O=Microsoft Corporation")
    );
    assert_eq!(result[0]["old_regex_matches"], true);
    assert_eq!(result[0]["organization"], "Example");
    assert_eq!(result[0]["accepted"], false);
    assert_eq!(result[1]["organization"], "Microsoft Corporation");
    assert_eq!(result[1]["accepted"], true);
    assert_eq!(result[2]["organization"], "");
    assert_eq!(result[2]["accepted"], false);
}

#[test]
fn microsoft_organization_still_requires_valid_authenticode_and_a_signer() {
    let result = fixture_result(
        r#"
$certificate=New-DerFixture 'CN=Runtime fixture, O=Microsoft Corporation'
try {
    $rows=@(foreach ($status in @('NotSigned','HashMismatch','NotTrusted','UnknownError')) {
        [pscustomobject]@{status=$status;accepted=(Test-SignatureAcceptance $certificate $status)}
    })
    $rows += [pscustomobject]@{status='ValidWithoutSigner';accepted=(Test-SignatureAcceptance $null 'Valid')}
    ConvertTo-Json -InputObject $rows -Compress
} finally { $certificate.Dispose() }
"#,
    );
    let rows = result.as_array().unwrap();
    assert_eq!(rows.len(), 5);
    for row in rows {
        assert_eq!(row["accepted"], false, "Invalid signature accepted: {row}");
    }
}

#[test]
fn signature_verification_uses_its_system_module_despite_inherited_module_path() {
    let before = r#"
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=New-Object Text.UTF8Encoding($false)
$temporary=[IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$root=$temporary+'lgsm-security-module-'+[Guid]::NewGuid().ToString('N')
if ([IO.Directory]::Exists($root)) { throw 'Fixture directory already exists' }
$moduleRoot=$root+'\Microsoft.PowerShell.Security'
[void][IO.Directory]::CreateDirectory($moduleRoot)
[IO.File]::WriteAllText($moduleRoot+'\Microsoft.PowerShell.Security.psd1',"@{ RootModule='Microsoft.PowerShell.Security.psm1'; ModuleVersion='99.0.0'; GUID='2ad945b6-aefa-4ed2-8cc9-1131ab1841fd'; FunctionsToExport='Get-AuthenticodeSignature' }")
[IO.File]::WriteAllText($moduleRoot+'\Microsoft.PowerShell.Security.psm1',"[IO.File]::WriteAllText((`$PSScriptRoot+'\shadow-loaded'),'yes'); function Get-AuthenticodeSignature { throw 'Untrusted shadow module loaded' }")
$original=$env:PSModulePath
$env:PSModulePath=$root+';'+$original
try {
"#;
    let after = r#"
Assert-MicrosoftSignature ($PSHOME+'\powershell.exe')
[pscustomobject]@{
    trusted_module=(Get-Module Microsoft.PowerShell.Security).Path.StartsWith($PSHOME+'\Modules\',[StringComparison]::OrdinalIgnoreCase)
    shadow_loaded=[IO.File]::Exists($moduleRoot+'\shadow-loaded')
}|ConvertTo-Json -Compress
} finally {
    $env:PSModulePath=$original
    if (![IO.Path]::GetFullPath($root).StartsWith($temporary,[StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture cleanup escaped its temporary directory' }
    [IO.Directory]::Delete($root,$true)
}
"#;
    let before_fix = process::SIGNATURE_FUNCTION.replace(
        "Import-Module -Name ($PSHOME+'\\Modules\\Microsoft.PowerShell.Security\\Microsoft.PowerShell.Security.psd1') -ErrorAction Stop",
        "",
    );
    let previous = capture_fixture_script(&[before, &before_fix, after].concat());
    assert!(
        !previous.status.success(),
        "The inherited module fixture must reproduce the old failure"
    );
    let output = capture_fixture_script(&[before, process::SIGNATURE_FUNCTION, after].concat());
    assert!(
        output.status.success(),
        "{}",
        output_excerpt(&output.stdout, &output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["trusted_module"], true);
    assert_eq!(result["shadow_loaded"], false);
}
