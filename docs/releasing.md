# Releasing VectorCraft

Every push to the `release` branch runs `.github/workflows/release.yml`. The workflow builds
Windows and Linux installers and portable packages, and creates or updates a
**draft** GitHub Release named `VectorCraft v<version>`. Nobody sees a draft until a maintainer
publishes it.

User-facing names say **VectorCraft**. Files, binaries and ids stay lowercase
(`vectorcraft-<version>-<platform>-<arch>.<ext>`, `io.github.treeforjs.vectorcraft`).

The application identifier is `io.github.treeforjs.vectorcraft`. Linux packages also retain the
older `ai.storyteller.vectorcraft` desktop-file and icon names as compatibility aliases. Its
desktop entry is hidden so application menus show one VectorCraft launcher. Native configuration
folders and the Windows MSI UpgradeCode stay unchanged, so existing settings and upgrades work.
The old macOS document type identifier remains an imported compatibility type. Flatpak users
changing application IDs should keep their previous installation and its data until they have
migrated their documents/settings; this release pipeline does not publish Flatpak packages.

## Cutting a release

1. **Bump the version** on `main`. It lives in exactly one place, `[workspace.package] version`
   in the root `Cargo.toml`; every crate inherits it and the packaging scripts read it from there:

   ```sh
   cargo xtask version                 # prints the current version, e.g. 0.3.1
   cargo xtask version set 0.4.0       # or 0.4.0-rc.1; updates Cargo.toml and Cargo.lock
   ```

   Commit the change (`Cargo.toml` + `Cargo.lock`) through the normal review flow, as a
   `Release: VectorCraft v0.4.0` commit whose message says what changed for users.
2. **Merge `main` into `release`** (or fast-forward it) and push. The workflow starts by itself.
3. **Wait for the draft.** When the Windows and Linux jobs are done, the
   Releases page has a draft `VectorCraft v0.4.0`, targeting the pushed commit, with every
   artifact, `SHA256SUMS.txt` and its Ed25519 signature `SHA256SUMS.txt.sig`.
4. **Check it.** Download and test a Windows portable ZIP and a Linux AppImage, and read the job
   summaries. Windows Authenticode is optional; update-signature verification is required.
5. **Publish** the draft in the GitHub UI. Publishing creates the `v0.4.0` tag. A version with a
   pre-release suffix (`-rc.1`) is marked as a pre-release.

Pushing to `release` again before you publish rebuilds the same draft and replaces its assets.
Once the draft is published, the workflow refuses to touch that version again: bump it first.

**Manual runs:** *Actions › Release › Run workflow* runs the pipeline by hand. Pick the
`release` branch to create a signed draft. The optional `version` input (such as `0.4.0-rc.1`)
overrides `Cargo.toml` for that run only; each build job applies it with `cargo xtask version set`
before building. Other branches cannot create or replace a draft. Every run validates the
repository public-key variable; release-branch runs also validate the private signing secret
and its public half before starting any package build. Jobs using the `release` environment
must be permitted by that environment's deployment-branch rules.

## What gets built

| Platform | Artifacts | Built on |
|---|---|---|
| Windows x64 | `vectorcraft-<v>-windows-x64.msi`, `vectorcraft-<v>-windows-x64-portable.zip` | `windows-latest` |
| Windows x86 (32-bit) | `vectorcraft-<v>-windows-x86.msi`, `vectorcraft-<v>-windows-x86-portable.zip` | `windows-latest` |
| Windows on ARM64 | `vectorcraft-<v>-windows-arm64.msi`, `vectorcraft-<v>-windows-arm64-portable.zip` | `windows-latest` (cross-compiled) |
| Linux x86_64 | `vectorcraft-<v>-linux-x86_64.{AppImage,deb,rpm,tar.gz}` | `ubuntu-22.04` |
| Linux aarch64 | `vectorcraft-<v>-linux-aarch64.{AppImage,deb,rpm,tar.gz}` | `ubuntu-22.04-arm` |
| Linux AppImage updates | `vectorcraft-<v>-linux-{x86_64,aarch64}.AppImage.zsync` | with the AppImages |

`vectorcraft --version` and `vectorcraft-cli --version` print the version from `Cargo.toml`.

Windows and Linux use standard title-bar controls. Linux enables winit's Adwaita fallback when
a Wayland compositor does not provide a frame; X11 and Windows retain their native decorations.
The Linux x86_64 release job launches the packaged build in a private Weston/Xvfb desktop and
uploads `window-frame-linux-x86_64` with its screenshot and logs for review. This diagnostic
artifact is separate from the signed release packages. To run the same check on Ubuntu 22.04+:

```sh
sudo apt-get install weston xvfb scrot libegl1-mesa libgl1-mesa-dri fonts-cantarell
bash packaging/linux/check-window-frame.sh target/release/vectorcraft target/window-frame
```

**Fonts.** Every build job checks out [craft-fonts](https://github.com/storytold/craft-fonts) at a
pinned commit (the `ref:` of its craft-fonts checkout step) and builds with `CRAFT_FONTS_DIR` and
`CRAFT_FONTS_REQUIRED=1`, so releases embed its Japanese fonts and fail rather than ship without
them. The packages carry each embedded font's licence as `OFL-<family>.txt`
(`copy_font_licences` in `packaging/env.sh`). Bump the pins deliberately, in every job at once.
The licence and source requirements are in [`AGENTS.md`](../AGENTS.md#non-negotiables),
and the build option is documented in [`development.md`](development.md) › Fonts.

The Linux window-frame dependency also embeds Cantarell Regular as a title-text fallback. Its
`OFL-Cantarell.txt` ships in Linux package documentation and in AppImages under
`usr/share/licenses/vectorcraft/`; no additional font file is stored in this repository.

### Windows

`packaging/windows/package.ps1 -Arch x64|x86|arm64` builds with `-C target-feature=+crt-static`,
so neither the MSI nor the portable zip needs the Visual C++ redistributable. The flag is scoped
to the target triple, so host build scripts aren't affected.

- Before packaging, the script checks each binary's PE machine type against `-Arch` (an x64 build
  can never ship labelled arm64), that `vectorcraft.exe` is a GUI program (no console window) and
  that `vectorcraft-cli.exe` is a console program.
- `vectorcraft.wxs` (WiX) is a per-machine install into Program Files with a Start Menu shortcut
  and an App Paths entry (Win+R `vectorcraft`). Same-version upgrades are allowed, so release
  candidates replace each other.
- Double-clicking the MSI opens a setup wizard (WixUI_InstallDir without the licence page):
  Welcome, install folder (remembered for upgrades in `HKLM\Software\VectorCraft\InstallDir`),
  Ready, a progress page, and a Finish page that says where to find the app, with a ticked "Launch
  VectorCraft" box. The pages name the full version (`vectorcraft.en-us.wxl`). `package.ps1` draws
  the wizard's banner and side bitmaps from the app icon, so no WiX stock art ships. `msiexec /qn`
  still installs silently. Building needs the WiX UI and Util extensions:
  `wix extension add -g WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2`.
- The ARM64 build is cross-compiled on the x64 runner. `.github/workflows/windows-arm64.yml`
  installs that MSI on a Windows 11 ARM64 runner, checks that both installed programs are ARM64,
  runs `vectorcraft-cli --version` natively and uninstalls.
- **Signing:** `packaging/windows/sign.ps1` signs both `.exe` files and then the `.msi` with
  `signtool` (SHA-256, RFC 3161 timestamp), using whichever material is present:
  1. a `.pfx` certificate (`WINDOWS_CERTIFICATE`, base64, and `WINDOWS_CERTIFICATE_PASSWORD`), or
  2. Azure Trusted Signing (`AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`,
     `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERT_PROFILE`).

  With neither, it warns and leaves the files unsigned, so test builds still produce installers.
  It is the one place to adapt when the signing setup changes; `WINDOWS_TIMESTAMP_URL` and
  `SIGNTOOL` override the timestamp server and the `signtool.exe` path.

### Linux

`packaging/linux/package.sh` builds the release binaries and packages them as an AppImage, a
`.deb` and an `.rpm` (with [nfpm](https://nfpm.goreleaser.com), from `nfpm.yaml`) and a plain
`.tar.gz` tree (`bin/`, `share/`). The packages install both programs, the desktop entry, the
AppStream metainfo, the MIME type for VectorCraft documents, the icons and the licence files.

The jobs run on `ubuntu-22.04`, the oldest GitHub-hosted image, so the binaries only need
glibc 2.35 or newer: Ubuntu 22.04+, Debian 12+, Fedora 36+ and RHEL 10. Moving the job to a newer
image raises that floor, so do it deliberately. After packaging, the job prints the `.deb`'s
metadata and contents, runs `ldd` on the binary and runs each AppImage with `--version`.
Local `.deb` builds derive their minimum `libc6` version from both packaged ELF binaries, so
building on a newer Ubuntu cannot silently advertise the release runner's older ABI floor.
The [Ubuntu compatibility workflow](../.github/workflows/ubuntu.yml) also builds and installs
packages on Ubuntu 24.04 and 26.04 x86_64, checks the native/legacy document MIME associations,
and runs the desktop smoke script under Xvfb with software graphics.

**AppImage updates.** Each AppImage embeds the update information
`gh-releases-zsync|treeforjs|vectorcraft|latest|vectorcraft-*-linux-<arch>.AppImage.zsync`
(appimagetool `-u`), and `package.sh` writes the matching `.zsync` beside it when `zsyncmake` (the
`zsync` package) is installed. [AppImageUpdate](https://github.com/AppImageCommunity/AppImageUpdate)
and AppImageLauncher then download only the changed blocks from the newest published,
non-pre-release release. The job checks both are there.

## The draft release

The last job waits for Windows and Linux, checks every expected package, writes `SHA256SUMS.txt`,
signs and verifies that list, and creates the draft `VectorCraft v<version>` with Windows/Linux
release notes. If the draft already exists, it replaces its assets and keeps it a draft. If that
version is already published, the job fails and asks for a version bump (`cargo xtask version set`).

## In-app updates

Help › Check for Updates… (and a daily background check, Preferences › General) looks for a newer
published release, downloads the file that matches how the copy was installed (`.msi`, AppImage,
`.deb`, `.rpm`), checks it against `SHA256SUMS.txt` and that list's Ed25519 signature, and installs
it when the app quits. The shared [`craft-update`](https://github.com/treeforjs/craftworks/tree/main/crates/craft-update)
crate does the work; the one-time key setup is in craftworks'
[`docs/updates.md`](https://github.com/treeforjs/craftworks/blob/main/docs/updates.md).

- The draft-release job requires `UPDATE_SIGNING_KEY` and signs `SHA256SUMS.txt` into
  `SHA256SUMS.txt.sig` (uploaded with the packages). Missing, malformed or mismatched keys fail
  the release; an unsigned draft is never created.
- Windows and Linux builds embed `CRAFT_UPDATE_PUBLIC_KEY`. Make it a **repository** variable
  (*Settings › Secrets and variables › Actions › Variables*), containing the single-line base64
  Ed25519 public key (raw32 or DER44). The Linux job does not run in the `release` environment, so an
  environment variable would not reach it. Use the same key pair as Craftworks.
- Only a published release is offered: drafts and (unless asked for) pre-releases never are.

## Secrets

The version preflight, Windows packaging and draft-release jobs use the `release` environment.
Allow the `release` branch in its deployment-branch rules. Set `UPDATE_SIGNING_KEY` there to the
complete, unencrypted Ed25519 private-key PEM, including its BEGIN/END lines. It is required for
release-branch runs and must match `CRAFT_UPDATE_PUBLIC_KEY`. Temporary private-key files use
restricted permissions and are removed when their steps exit. Windows certificate secrets are
optional and separate from the required update signature.

| Secret | Used for |
|---|---|
| `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | Windows signing with a `.pfx` |
| `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERT_PROFILE` | Windows signing with Azure Trusted Signing |
| `UPDATE_SIGNING_KEY` | the Ed25519 private key (PEM) that signs `SHA256SUMS.txt` for in-app updates (see [In-app updates](#in-app-updates)) |
