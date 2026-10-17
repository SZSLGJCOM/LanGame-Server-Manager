//! The installer allowlist is deliberately independent of game packages.
//! Sources: Microsoft Download Center (27598, 8109) and the Microsoft Learn
//! latest-supported-vc-redist page. Never execute a game's installscript.
use app_runtime::{WindowsRuntimeArchitecture as Architecture, WindowsRuntimeFamily as Family};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InstallerKind {
    XnaMsi,
    VisualCpp,
    DirectX,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RuntimePackage {
    pub id: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: Option<&'static str>,
    pub kind: InstallerKind,
    pub rolling: bool,
}

pub(super) const XNA: RuntimePackage = RuntimePackage {
    id: "xna-4-refresh",
    name: "Microsoft XNA Framework 4.0 Refresh",
    url: "https://download.microsoft.com/download/5/3/A/53A804C8-EC78-43CD-A0F0-2FB4D45603D3/xnafx40_redist.msi",
    // Microsoft's winget-pkgs XNARedist 4.0.30901.0 manifest.
    sha256: Some("2b03c130c4bb5f106b9619bea8150201f7e982709cfe9a0cc8bde75ff0a83b27"),
    kind: InstallerKind::XnaMsi,
    rolling: false,
};

pub(super) fn package(family: Family, architecture: Architecture) -> RuntimePackage {
    let (id, name, url) = match (family, architecture) {
        (Family::VisualCpp2010, Architecture::X86) => (
            "vc-2010-x86",
            "Microsoft Visual C++ 2010 SP1 (x86)",
            "https://download.microsoft.com/download/1/6/5/165255E7-1014-4D0A-B094-B6A430A6BFFC/vcredist_x86.exe",
        ),
        (Family::VisualCpp2010, Architecture::X64) => (
            "vc-2010-x64",
            "Microsoft Visual C++ 2010 SP1 (x64)",
            "https://download.microsoft.com/download/1/6/5/165255E7-1014-4D0A-B094-B6A430A6BFFC/vcredist_x64.exe",
        ),
        (Family::VisualCpp2012, Architecture::X86) => (
            "vc-2012-x86",
            "Microsoft Visual C++ 2012 Update 4 (x86)",
            "https://download.microsoft.com/download/1/6/B/16B06F60-3B20-4FF2-B699-5E9B7962F9AE/VSU_4/vcredist_x86.exe",
        ),
        (Family::VisualCpp2012, Architecture::X64) => (
            "vc-2012-x64",
            "Microsoft Visual C++ 2012 Update 4 (x64)",
            "https://download.microsoft.com/download/1/6/B/16B06F60-3B20-4FF2-B699-5E9B7962F9AE/VSU_4/vcredist_x64.exe",
        ),
        (Family::VisualCpp2013, Architecture::X86) => (
            "vc-2013-x86",
            "Microsoft Visual C++ 2013 Update 5 (x86)",
            "https://download.visualstudio.microsoft.com/download/pr/10912113/5da66ddebb0ad32ebd4b922fd82e8e25/vcredist_x86.exe",
        ),
        (Family::VisualCpp2013, Architecture::X64) => (
            "vc-2013-x64",
            "Microsoft Visual C++ 2013 Update 5 (x64)",
            "https://download.visualstudio.microsoft.com/download/pr/10912041/cee5d6bca2ddbcd039da727bf4acb48a/vcredist_x64.exe",
        ),
        (Family::VisualCpp2015, Architecture::X86) => (
            "vc-14-x86",
            "Microsoft Visual C++ v14 (x86)",
            "https://aka.ms/vc14/vc_redist.x86.exe",
        ),
        (Family::VisualCpp2015, Architecture::X64) => (
            "vc-14-x64",
            "Microsoft Visual C++ v14 (x64)",
            "https://aka.ms/vc14/vc_redist.x64.exe",
        ),
        (Family::DirectXJune2010, _) => (
            "directx-june-2010",
            "Microsoft DirectX End-User Runtimes (June 2010)",
            "https://download.microsoft.com/download/8/4/A/84A35BF1-DAFE-4AE8-82AF-AD2AE20B6B14/directx_Jun2010_redist.exe",
        ),
    };
    RuntimePackage {
        id,
        name,
        url,
        sha256: None,
        kind: if family == Family::DirectXJune2010 {
            InstallerKind::DirectX
        } else {
            InstallerKind::VisualCpp
        },
        rolling: family == Family::VisualCpp2015,
    }
}
