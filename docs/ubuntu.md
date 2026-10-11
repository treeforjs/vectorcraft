# Ubuntu 24.04 / 26.04 LTS

These instructions target x86_64 (Intel/AMD 64-bit) Ubuntu Desktop. VectorCraft supports
native Wayland and X11; Ubuntu's normal desktop session supplies the display server and GPU
drivers. A server installation can use `vectorcraft-cli` without a desktop.

## Install a package

Download the x86_64 `.deb` from the release you want, check it against that release's
`SHA256SUMS.txt`, then install the local file with APT so its dependencies are installed too:

```sh
sudo apt update
sudo apt install ./vectorcraft-<ver>-linux-x86_64.deb
vectorcraft --version
vectorcraft
```

Replace `<ver>` with the downloaded version. The package installs both `vectorcraft` and
`vectorcraft-cli`, an application-menu launcher, icons and file associations for `.vectorcraft`,
legacy `.drawcraft`, SVG/SVGZ and PDF. Right-click a document and choose **Open With VectorCraft**;
installation does not replace your default PDF viewer.

Packages built on Ubuntu 24.04 also run on 26.04. A package built on 26.04 may require a newer
glibc and must not be assumed to run on 24.04. Official release builds retain their Ubuntu 22.04
baseline (glibc 2.35); build your own distributable on the oldest Ubuntu you intend to support.

For an AppImage, make it executable and run it. If FUSE mounting is unavailable, use
`APPIMAGE_EXTRACT_AND_RUN=1 ./vectorcraft-<ver>-linux-x86_64.AppImage`; this requires no FUSE
installation. The `.deb` is the simplest choice for application-menu and file associations.

## Build this checkout

Install Rust with [rustup](https://rustup.rs/). The workspace requires Rust **1.95 or newer**;
the `rustc` package in an older Ubuntu release may be too old. Then, from this checkout:

```sh
sudo apt update
sudo apt install --no-install-recommends build-essential pkg-config cmake git curl \
  libxkbcommon-dev libxkbcommon-x11-0 libwayland-dev libx11-dev libxrandr-dev libxi-dev \
  libgl-dev libegl-dev libegl1 libegl-mesa0 libgl1-mesa-dri libvulkan1 mesa-vulkan-drivers \
  xdg-desktop-portal xdg-desktop-portal-gtk fonts-dejavu-core
rustup update stable
cargo build --release --locked -p vectorcraft -p vectorcraft-cli
./target/release/vectorcraft
```

On a machine with little RAM, add `CARGO_BUILD_JOBS=2` before the Cargo command. Builds can use
several gigabytes of disk. System fonts work without the optional craft-fonts checkout.
For printing to installed CUPS printers, also install `cups-client`.

To make an installable package, install the pinned nFPM version used in
[release.yml](../.github/workflows/release.yml) from [nFPM](https://nfpm.goreleaser.com/install/),
then run:

```sh
packaging/linux/package.sh --skip-build --formats "deb tar"
sudo apt install ./dist/release/vectorcraft-*-linux-x86_64.deb
```

`CARGO_TARGET_DIR` is supported by both Cargo and the packaging script. The tarball contains
`bin/` and `share/`; it can be unpacked anywhere and run as `bin/vectorcraft`.

## Graphics hardware

The priority workstation targets are **NVIDIA RTX PRO 4000 Blackwell** and
**RTX PRO 2000 Blackwell**. GeForce RTX 30/40/50-series cards and Intel integrated graphics
are also targets. These are driver/setup targets; the automated checks use Mesa software
rendering and do not certify these physical GPUs.

| Hardware | Ubuntu graphics stack |
|---|---|
| RTX PRO 4000 / 2000 Blackwell, GeForce RTX 50 series | Ubuntu's recommended NVIDIA desktop driver, with the **open kernel modules** required by Blackwell |
| GeForce RTX 30 / 40 series | Ubuntu's recommended NVIDIA desktop driver |
| Intel integrated graphics | Ubuntu's kernel, Mesa Vulkan/OpenGL drivers and `linux-firmware` |

For NVIDIA, install available Ubuntu updates, then let Ubuntu select a driver for the actual
GPU instead of pinning a driver branch:

```sh
sudo apt update
sudo apt upgrade
sudo apt install ubuntu-drivers-common
ubuntu-drivers list
sudo ubuntu-drivers install
sudo reboot
```

For Blackwell, the selected driver package must use the `-open` kernel-module flavor. After
rebooting, check `nvidia-smi`; `modinfo -F license nvidia` should report `Dual MIT/GPL` for the
open modules. If Secure Boot requests key enrollment during installation, complete it at reboot.
The [NVIDIA supported-GPU list](https://download.nvidia.com/XFree86/Linux-x86_64/595.84/README/supportedchips.html)
includes both priority workstation cards;
[NVIDIA's kernel-module documentation](https://download.nvidia.com/XFree86/Linux-x86_64/580.95.05/README/kernel_open.html)
explains the Blackwell requirement. See also
[Ubuntu's desktop driver instructions](https://ubuntu.com/desktop/docs/en/latest/how-to/graphics/install-nvidia-drivers/).

For Intel, use Ubuntu's maintained kernel and Mesa packages:

```sh
sudo apt update
sudo apt upgrade
sudo apt install libvulkan1 mesa-vulkan-drivers libegl1 libegl-mesa0 libgl1 libgl1-mesa-dri linux-firmware
```

On Ubuntu 24.04 with a recent Intel integrated GPU, install the current
[HWE kernel](https://ubuntu.com/kernel/docs/reference/hwe-kernels/) and reboot:
`sudo apt install --install-recommends linux-generic-hwe-24.04`. Ubuntu 26.04 supplies a newer
kernel and graphics stack directly. See [Mesa's supported systems](https://docs.mesa3d.org/systems.html)
and its [Intel runtime dependencies](https://docs.mesa3d.org/drivers/anv.html#runtime-dependencies),
plus [Intel's Ubuntu compatibility guidance](https://dgpu-docs.intel.com/installation-guides/installing-packages-from-the-intel-ppa.html).

VectorCraft normally selects the GPU driving a display. **Help → About** reports the adapter
in use; **Preferences → Performance → Graphics Processor** changes the preference for the next
launch. On hybrid systems, start with Automatic. The canvas is rendered on the CPU; the GPU
draws the window and composites the canvas.

## Desktop troubleshooting

- **File dialogs do not open:** run inside your logged-in desktop session, with its D-Bus
  session and `xdg-desktop-portal` service. The GTK portal backend above supplies the chooser
  on Ubuntu GNOME. KDE users should use their desktop's `xdg-desktop-portal-kde` backend.
- **File drag-and-drop or a drawing tablet fails on Wayland:** current winit lacks these
  Wayland protocols. Use File → Open / Place, or run `env -u WAYLAND_DISPLAY vectorcraft`
  through XWayland. Ubuntu GNOME's native Wayland clipboard also has limitations for artwork
  copied from other apps; see [Wayland and X11](development.md#linux-wayland-and-x11).
- **A graphics driver fails:** try `WGPU_BACKEND=gl vectorcraft`. On Mesa, the software fallback
  `LIBGL_ALWAYS_SOFTWARE=1 WGPU_BACKEND=gl vectorcraft` can help diagnose GPU problems.
- **No window on SSH or Ubuntu Server:** the GUI needs a display session. Use
  `vectorcraft-cli convert input.svg output.png` for headless conversion.

Startup diagnostics are in `~/.config/vectorcraft/logs/vectorcraft.log` (or under
`$XDG_CONFIG_HOME/vectorcraft/logs`).

## Automated Ubuntu checks

The [Ubuntu workflow](../.github/workflows/ubuntu.yml) builds and installs a `.deb` in Ubuntu
24.04 and 26.04 x86_64 containers. It validates the launcher and MIME registrations, converts a
small SVG to PNG/PDF, and drives a real window under Xvfb with Mesa software rendering, including
an edit, save and screenshot. Run the same smoke check in a desktop session with
`packaging/linux/smoke.sh`, or under `xvfb-run -a dbus-run-session -- packaging/linux/smoke.sh`.
It needs `jq`, `xvfb`, `xauth` and `dbus-x11` for the headless form.

On 2026-10-11, local checks also installed the package on both Ubuntu versions and exercised
edit/save/reopen and screenshots as a second, unprivileged user under X11 and headless Weston
Wayland. The launcher, icons, documentation and native/legacy MIME associations were checked.

The software-rendered X11 check does not cover physical GPUs, native GNOME Wayland, tablets,
clipboard integration with other apps, printing or interactive portal dialogs.
