# SQLx SQLite driver

This directory contains `sqlx-sqlite 0.9.0` from the published
[crates.io archive](https://static.crates.io/crates/sqlx-sqlite/sqlx-sqlite-0.9.0.crate).
The archive SHA-256 is
`488e99c397a62007e4229aec669a179816339afc6d2620ca6fa420dbee2e982c`;
its recorded upstream revision is
`003b698e99e024f3621b8043a2426fde5b741171`.

All Rust source and both upstream license files are unchanged. The archive's
standalone `Cargo.lock`, original workspace manifest (`Cargo.toml.orig`) and
`.cargo_vcs_info.json` are omitted; the application workspace owns resolution.

The only build change is the `libsqlite3-sys` dependency range in the normalized
`Cargo.toml`: `>=0.30.1, <0.38.0` becomes `>=0.30.1, <0.40.0`.
This incorporates the dependency-bound changes from upstream commits
[`94aafe3a`](https://github.com/launchbadge/sqlx/commit/94aafe3a68884d923b0798a767c8d7f6cfda89d2)
and
[`10462fa4`](https://github.com/launchbadge/sqlx/commit/10462fa4414c6c7f1ed1d39bb31678e3b726878c).
It permits the workspace to use `libsqlite3-sys 0.39.1` (SQLite 3.54.1), fixing
[RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html), while
retaining the published SQLx 0.9.0 driver implementation and `sqlx-core` API.

The driver remains available under its upstream MIT or Apache-2.0 licenses;
the LanGame project license does not replace these terms. Both complete license
texts and this source notice accompany the installed application under
`third-party/sqlx-sqlite/`.

Remove this directory and the workspace patch when a published SQLx release
supports a patched SQLite dependency. Update the shared SQLx version, regenerate
the lock file and notices, and verify storage tests and the Rust security check
before removing the patch.

本目录保留 SQLx 0.9.0 原始驱动源码及许可，仅移植上游已合并的依赖版本上限修正，
以使用修复安全问题的 SQLite。正式发布的 SQLx 支持安全版本后，应移除此临时补丁；
应用安装包随附本来源说明及完整上游许可。
