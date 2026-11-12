# Mod archive decoder fixtures

These small, unchanged public test archives were copied from the local Cargo
registry source packages, not from a user's downloads or profile. `sources.json`
records the exact package, source path, size and SHA-256 for each file.

- `sevenz-rust2` 0.23.0: https://github.com/hasenbanck/sevenz-rust2, Apache-2.0;
  license reproduced in `LICENSE-sevenz-APACHE`. The production reader is pinned
  separately to 0.25.0. LZMA, solid and PPMd samples exercise real codec data.
- `unrar` 0.5.8: https://github.com/muja/unrar.rs, MIT OR Apache-2.0;
  both licenses reproduced. `rar4.rar`, `rar5-solid.rar` and `rar5-unicode.rar`
  exercise upstream's RAR4/RAR5/Unicode cases. Encrypted examples must fail
  without prompting or writing any payload.

`LICENSE-unrar-rs` additionally records the production RAR reader's UnRAR license
notice from the pinned `unrar-rs` 0.15.3 package. It permits reading/extracting RAR
archives and restricts implementing a compatible RAR archiver. This application
only decodes; it does not create RAR archives.

Other test cases construct tiny stored RAR4 containers and 7z archives entirely
inside isolated temporary directories to cover transaction paths and hostile
metadata. They contain test text only; no executable fixture is run.
