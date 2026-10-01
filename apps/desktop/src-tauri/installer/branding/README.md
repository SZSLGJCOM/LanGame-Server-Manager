# NSIS branding inputs

`header.bmp` (150 x 57) is shared by installation and uninstallation. `sidebar.bmp` (164 x 314) is used on the welcome and finish pages. Both are opaque, uncompressed, 24-bit Windows bitmaps at NSIS's native dimensions.

The matching SVGs are self-contained vector sources generated from the existing complete `apps/desktop/src/assets/langame-logo.svg` and `langame-logo-dark.svg`. The generator preserves their path geometry, uses a monochrome palette, and records each canonical logo's normalized SHA-256 in the SVG. No additional font or third-party artwork is required. Edit the canonical logo or the generator's layout rather than modifying a generated SVG/BMP independently.

From the repository root, use an installed Chrome/Edge and an external work directory:

```powershell
node scripts/generate_desktop_installer_branding.cjs --work-dir C:\ReleaseStaging\branding
node scripts/generate_desktop_installer_branding.cjs --work-dir C:\ReleaseStaging\branding --check
```

On a managed development host, run those Node arguments through its required process guard with the `e2e` profile. `--browser <executable>` selects an installed Chromium browser explicitly; otherwise the existing `LANGAME_RELIABILITY_BROWSER` override or standard Windows Chrome/Edge locations are used.

The generator uses only Node's standard library and the installed browser. `--check` renders again and compares the complete SVG and BMP bytes without rewriting the inputs. Chromium rasterization can change across browser versions, so use the same renderer for byte-for-byte reproduction. The work directory receives a 3x nearest-neighbor pixel preview and a renderer-version receipt; temporary browser profiles are removed. These files are previews of the supplied images, not screenshots of the actual installer wizard.
