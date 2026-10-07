from __future__ import annotations


AUDITED_SOURCES = {
    "spdx-apache-2.0": {
        "path": "SPDX-Apache-2.0.txt",
        "sha256": "074e6e32c86a4c0ef8b3ed25b721ca23aca83df277cd88106ef7177c354615ff",
        "url": "https://github.com/spdx/license-list-data/blob/v3.27.0/text/Apache-2.0.txt",
    },
    "spdx-mit": {
        "path": "SPDX-MIT.txt",
        "sha256": "b05785f9f18e6716bab63424b11454513b9943a222595b70411009202fc592b5",
        "url": "https://github.com/spdx/license-list-data/blob/v3.27.0/text/MIT.txt",
    },
    "spdx-mpl-2.0": {
        "path": "SPDX-MPL-2.0.txt",
        "sha256": "66a3107d5ad6a058aab753eaac2047ccb2ed0e39465dd0fe5844da3e300d5172",
        "url": "https://github.com/spdx/license-list-data/blob/v3.27.0/text/MPL-2.0.txt",
    },
    "objc2-license-notice": {
        "path": "objc2-7f976f7e-LICENSE.md",
        "sha256": "7f976f7e9cb2d87df7230606feb932c3f21ac0e664045a775b600046ff850c54",
        "url": "https://github.com/madsmtm/objc2/blob/8852b424193ca41602281b3d7540d7c8ed51e49a/LICENSE.md",
    },
    "dlopen2-mit": {
        "path": "dlopen2-cc80e4a0-LICENSE",
        "sha256": "39fa265207450e77c62e90c5594a06c085b655d8374c7ced4bf7894b6bd95dd2",
        "url": "https://github.com/OpenByteDev/dlopen2/blob/cc80e4a0a90d499b677fdf7743699b4b3a43a989/LICENSE",
    },
    "webview2-mit": {
        "path": "webview2-b74dc5e2-LICENSE",
        "sha256": "0dcf41516e608bbcb6cdc5229feb7b86fe4a643b85e7df251133c93408fdac73",
        "url": "https://github.com/wravery/webview2-rs/blob/b74dc5e2b394044bea5191052868ce7a106c202c/LICENSE",
    },
}


def audited(source_id: str) -> dict[str, str]:
    return {"kind": "audited", "source_id": source_id}


def archive_member(member_name: str) -> dict[str, str]:
    return {"kind": "archive-member", "member_name": member_name}


def sibling(package_id: str, member_name: str) -> dict[str, str]:
    return {
        "kind": "sibling",
        "package_id": package_id,
        "member_name": member_name,
    }


MISSING_LICENSE_RULES: dict[str, dict[str, object]] = {}


def add_rule(
    package_id: str,
    *,
    declared_license: str,
    selected_license: str,
    repository: str | None,
    vcs_sha1: str | None,
    sources: list[dict[str, str]],
) -> None:
    if package_id in MISSING_LICENSE_RULES:
        raise ValueError(f"duplicate Rust license rule for {package_id}")
    MISSING_LICENSE_RULES[package_id] = {
        "declared_license": declared_license,
        "selected_license": selected_license,
        "repository": repository,
        "vcs_sha1": vcs_sha1,
        "sources": sources,
    }


# These checksum-locked manifests explicitly declare MIT and identify Jeff
# Muizelaar as author. Their exact upstream commits also omit standalone legal
# files; preserve the manifest attribution in the inventory and use the already
# audited SPDX MIT text, without inventing a copyright year or ownership claim.
for crate_name, version, vcs_sha1 in (
    ("adobe-cmap-parser", "0.4.1", "ac107d55f0d31a9d47955082238ab9e4fb157cdb"),
    ("pdf-extract", "0.12.0", "b95bf9f6268772d5088f09b0034e488e64294835"),
    ("type1-encoding-parser", "0.1.1", "55d193e28c9a5909be02a5b1f91fd3f76b076747"),
):
    add_rule(
        f"{crate_name}@{version}",
        declared_license="MIT",
        selected_license="MIT",
        repository=f"https://github.com/jrmuizel/{crate_name}",
        vcs_sha1=vcs_sha1,
        sources=[audited("spdx-mit")],
    )


add_rule(
    "alloc-stdlib@0.3.0",
    declared_license="BSD-3-Clause",
    selected_license="BSD-3-Clause",
    repository="https://github.com/dropbox/rust-alloc-no-stdlib",
    vcs_sha1="0a81fd6928ea3b33c8cd484aa4575d50ffb98012",
    sources=[sibling("alloc-no-stdlib@3.0.0", "LICENSE")],
)
add_rule(
    "cesu8@1.1.0",
    declared_license="Apache-2.0/MIT",
    selected_license="Apache-2.0",
    repository="https://github.com/emk/cesu8-rs",
    vcs_sha1=None,
    sources=[audited("spdx-apache-2.0")],
)
add_rule(
    "crc-catalog@2.5.0",
    declared_license="MIT OR Apache-2.0",
    selected_license="Apache-2.0",
    repository="https://github.com/akhilles/crc-catalog.git",
    vcs_sha1="ed4ad631f22b05055c21a3a4127eb7cf6d75bb62",
    sources=[audited("spdx-apache-2.0")],
)
add_rule(
    "defmt-parser@1.0.0",
    declared_license="MIT OR Apache-2.0",
    selected_license="Apache-2.0",
    repository="https://github.com/knurling-rs/defmt",
    vcs_sha1="4a8cdb44891ed57b8ff5a023b6bec7137c48708f",
    sources=[sibling("defmt@1.1.1", "LICENSE-APACHE")],
)
for package_id in ("dlopen2@0.8.2", "dlopen2_derive@0.4.3"):
    add_rule(
        package_id,
        declared_license="MIT",
        selected_license="MIT",
        repository="https://github.com/OpenByteDev/dlopen2",
        vcs_sha1="cc80e4a0a90d499b677fdf7743699b4b3a43a989",
        sources=[audited("dlopen2-mit")],
    )
add_rule(
    "jni-sys-macros@0.4.1",
    declared_license="MIT OR Apache-2.0",
    selected_license="Apache-2.0",
    repository="https://github.com/jni-rs/jni-sys",
    vcs_sha1="64d77b7a5f119d7b55b4e2c169a4668067ff59e6",
    sources=[sibling("jni-sys@0.4.1", "LICENSE-APACHE")],
)

for package_id, vcs_sha1 in (
    ("jni@0.22.4", "5ae9458a4ec44c5318f37ddc7569c1d4ae8a69e7"),
    ("jni-macros@0.22.4", "33045a124105c939d1e2cbdcb5a39e5d868ffa03"),
):
    add_rule(
        package_id,
        declared_license="MIT OR Apache-2.0",
        selected_license="Apache-2.0",
        repository="https://github.com/jni-rs/jni-rs",
        vcs_sha1=vcs_sha1,
        sources=[audited("spdx-apache-2.0")],
    )
add_rule(
    "libappindicator-sys@0.9.0",
    declared_license="Apache-2.0 OR MIT",
    selected_license="Apache-2.0",
    repository=None,
    vcs_sha1="eafd1e3682a1247f595410266091e9684021cb6f",
    sources=[sibling("libappindicator@0.9.0", "LICENSE-APACHE")],
)
add_rule(
    "mlua-sys@0.12.0",
    declared_license="MIT",
    selected_license="MIT",
    repository="https://github.com/mlua-rs/mlua",
    vcs_sha1="4fd87af2157b0a7ecd22ba299848e4ca3d462efe",
    sources=[sibling("mlua@0.12.1", "LICENSE")],
)
add_rule(
    "ndk-context@0.1.1",
    declared_license="MIT OR Apache-2.0",
    selected_license="Apache-2.0",
    repository="https://github.com/rust-windowing/android-ndk-rs",
    vcs_sha1="10f2ba388fca20f7349996ebae26ccda7a6fda5c",
    sources=[audited("spdx-apache-2.0")],
)
for package_id in ("ndk@0.9.0", "ndk-sys@0.6.0+11769913"):
    add_rule(
        package_id,
        declared_license="MIT OR Apache-2.0",
        selected_license="Apache-2.0",
        repository="https://github.com/rust-mobile/ndk",
        vcs_sha1="49bbbba16c58ff63cb8a0ad0eca5a9fb7ecaec25",
        sources=[audited("spdx-apache-2.0")],
    )


OBJC_NOTICE = audited("objc2-license-notice")
OBJC_APACHE_SOURCES = [audited("spdx-apache-2.0"), OBJC_NOTICE]
OBJC_MIT_SOURCES = [audited("spdx-mit"), OBJC_NOTICE]

add_rule(
    "block2@0.6.2",
    declared_license="MIT",
    selected_license="MIT",
    repository="https://github.com/madsmtm/objc2",
    vcs_sha1="b4167b582b2f75f9a1be75495c41b765344fd03c",
    sources=OBJC_MIT_SOURCES,
)
add_rule(
    "dispatch2@0.3.1",
    declared_license="Zlib OR Apache-2.0 OR MIT",
    selected_license="Apache-2.0",
    repository="https://github.com/madsmtm/objc2",
    vcs_sha1="8852b424193ca41602281b3d7540d7c8ed51e49a",
    sources=OBJC_APACHE_SOURCES,
)
add_rule(
    "objc2@0.6.4",
    declared_license="MIT",
    selected_license="MIT",
    repository="https://github.com/madsmtm/objc2",
    vcs_sha1="8852b424193ca41602281b3d7540d7c8ed51e49a",
    sources=OBJC_MIT_SOURCES,
)
for crate_name in (
    "objc2-app-kit",
    "objc2-cloud-kit",
    "objc2-core-data",
    "objc2-core-foundation",
    "objc2-core-graphics",
    "objc2-core-image",
    "objc2-core-location",
    "objc2-core-text",
    "objc2-io-surface",
    "objc2-osa-kit",
    "objc2-quartz-core",
    "objc2-ui-kit",
    "objc2-user-notifications",
    "objc2-web-kit",
):
    add_rule(
        f"{crate_name}@0.3.2",
        declared_license="Zlib OR Apache-2.0 OR MIT",
        selected_license="Apache-2.0",
        repository="https://github.com/madsmtm/objc2",
        vcs_sha1="7b1abfd750a2cacaea71d6a56ecfb83cb7de560b",
        sources=OBJC_APACHE_SOURCES,
    )
add_rule(
    "objc2-foundation@0.3.2",
    declared_license="MIT",
    selected_license="MIT",
    repository="https://github.com/madsmtm/objc2",
    vcs_sha1="7b1abfd750a2cacaea71d6a56ecfb83cb7de560b",
    sources=OBJC_MIT_SOURCES,
)
add_rule(
    "objc2-encode@4.1.0",
    declared_license="MIT",
    selected_license="MIT",
    repository="https://github.com/madsmtm/objc2",
    vcs_sha1="8d214f5477365ffcbcbb7de058c86ed9a518efb7",
    sources=OBJC_MIT_SOURCES,
)
add_rule(
    "objc2-exception-helper@0.1.1",
    declared_license="Zlib OR Apache-2.0 OR MIT",
    selected_license="Apache-2.0",
    repository="https://github.com/madsmtm/objc2",
    vcs_sha1="8d214f5477365ffcbcbb7de058c86ed9a518efb7",
    sources=OBJC_APACHE_SOURCES,
)

for package_id, vcs_sha1 in (
    ("r-efi@5.3.0", "97b55bed1c2c91dcbf787674849f05337ff80b33"),
    ("r-efi@6.0.0", "7e1b0322d31d625f81a5656096330934f9cd835d"),
):
    add_rule(
        package_id,
        declared_license="MIT OR Apache-2.0 OR LGPL-2.1-or-later",
        selected_license="MIT",
        repository="https://github.com/r-efi/r-efi",
        vcs_sha1=vcs_sha1,
        sources=[archive_member("AUTHORS")],
    )
add_rule(
    "rustls-platform-verifier-android@0.1.1",
    declared_license="MIT OR Apache-2.0",
    selected_license="Apache-2.0",
    repository="https://github.com/rustls/rustls-platform-verifier",
    vcs_sha1=None,
    sources=[sibling("rustls-platform-verifier@0.7.0", "LICENSE-APACHE")],
)
for package_id, vcs_sha1 in (
    ("selectors@0.36.1", "635e1a19d02960588a00e189bd4bd5bdb150ec3d"),
    ("selectors@0.38.0", "572ecba2d1600e7c3d490586692a209faf703baa"),
):
    add_rule(
        package_id,
        declared_license="MPL-2.0",
        selected_license="MPL-2.0",
        repository="https://github.com/servo/stylo",
        vcs_sha1=vcs_sha1,
        sources=[audited("spdx-mpl-2.0")],
    )
add_rule(
    "tauri-plugin@2.6.3",
    declared_license="Apache-2.0 OR MIT",
    selected_license="Apache-2.0",
    repository="https://github.com/tauri-apps/tauri",
    vcs_sha1="6f6ab1207bb3923c2721fbc67d2fdb1c8deb0c7a",
    sources=[sibling("tauri-build@2.7.1", "LICENSE-APACHE-2.0")],
)
for package_id, vcs_sha1 in (
    ("webview2-com@0.39.1", "edc2caf886175ccaebe86078c9cfe1ae2a187328"),
    ("webview2-com-sys@0.39.1", "edc2caf886175ccaebe86078c9cfe1ae2a187328"),
    ("webview2-com-macros@0.8.1", "dffa41a8a46d3f5565eefbff2de57d38d399f158"),
):
    add_rule(
        package_id,
        declared_license="MIT",
        selected_license="MIT",
        repository="https://github.com/wravery/webview2-rs",
        vcs_sha1=vcs_sha1,
        sources=[audited("webview2-mit")],
    )
for package_id in (
    "winapi-i686-pc-windows-gnu@0.4.0",
    "winapi-x86_64-pc-windows-gnu@0.4.0",
):
    add_rule(
        package_id,
        declared_license="MIT/Apache-2.0",
        selected_license="Apache-2.0",
        repository="https://github.com/retep998/winapi-rs",
        vcs_sha1=None,
        sources=[sibling("winapi@0.3.9", "LICENSE-APACHE")],
    )
