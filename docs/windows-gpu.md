# Windows graphics setup

This guide targets **Windows 11 x64**, prioritizing **NVIDIA RTX PRO 4000 Blackwell** and
**RTX PRO 2000 Blackwell**, plus GeForce RTX 30/40/50-series cards and Intel integrated graphics.
The driver guidance comes from the hardware vendors. Windows execution and these physical GPUs
have not been validated in the Ubuntu compatibility test environment.

## Choose the driver for your GPU

| GPU | Driver choice |
|---|---|
| RTX PRO 4000 / 2000 Blackwell | Current NVIDIA RTX Enterprise Production Branch for the exact desktop or laptop model and Windows 11 64-bit |
| GeForce RTX 30 / 40 / 50 series | NVIDIA Studio Driver for creative workloads; Game Ready is also an option when gaming updates are the priority |
| Intel integrated graphics | The PC maker's graphics driver first; Intel Driver & Support Assistant can identify the appropriate generic package for the processor/graphics generation |

Use the [official NVIDIA driver selector](https://www.nvidia.com/en-us/drivers/), and check the
selected download's supported-products list before installation. The verified
[RTX Enterprise list](https://www.nvidia.com/en-us/drivers/details/279889/) includes both priority
workstation cards; the [Studio list](https://www.nvidia.com/en-us/drivers/details/280101/) includes
GeForce RTX 30/40/50 families. These establish coverage, not a minimum or pinned driver version.
NVIDIA explains [Studio versus Game Ready](https://www.nvidia.com/en-us/geforce/news/ces-2022-nvidia-community-qa/).

For Intel, follow its [OEM-first recommendation](https://www.intel.com/content/www/us/en/support/articles/000058295/graphics.html)
and [driver-generation / Driver & Support Assistant guidance](https://www.intel.com/content/www/us/en/support/articles/000090440/graphics.html).
Arc/Core Ultra, earlier Iris Xe/UHD generations and older integrated graphics use different
driver branches; choose the one covering the actual processor.

Install Windows updates and the selected vendor driver, then reboot before testing VectorCraft.
On a hybrid laptop, keep both the integrated and discrete GPU drivers current. Use the normal
signed Windows driver package for the exact GPU and OS.

## Check the adapter VectorCraft uses

Start with **Preferences → Performance → Graphics Processor → Automatic**. VectorCraft selects
a GPU that can present to the window and prefers the one driving the primary display. The
canvas is rasterized on the CPU; the GPU composites the canvas and draws the interface.

**Help → About** reports the adapter in use. Startup diagnostics are saved to
`%APPDATA%\VectorCraft\logs\vectorcraft.log`, including the GPUs found, the chosen GPU and backend,
and any graphics-device failure. NVIDIA users can also run `nvidia-smi`; Windows Device Manager
shows the installed display adapters and driver versions.
Run `dxdiag` and inspect the Display/Render tabs to record the GPU and driver versions. If
Windows lists [Microsoft Basic Display Adapter](https://support.microsoft.com/en-us/windows/hardware/display-graphics/microsoft-basic-display-adapter-in-windows),
install the proper vendor or OEM driver before evaluating graphics performance.

Windows also exposes a per-app preference in **Settings → System → Display → Graphics →
VectorCraft → Options** ([Microsoft instructions](https://support.microsoft.com/en-us/windows/hardware/display-graphics/optimizations-for-windowed-games-in-windows-11)).
Save changes and restart the app, then check its reported adapter: VectorCraft has its own
adapter selection, so verify the result instead of assuming the OS preference selected it.

Windows normally uses **Direct3D 12**, with **OpenGL** as the fallback. Vulkan is enabled only
when explicitly requested because some Windows Vulkan drivers have failed during startup.
See the [graphics implementation notes](development.md#desktop-graphics-processor).

If the window is black or fails to start after installing the current driver, try the OpenGL
backend from PowerShell. This example uses the default x64 installation location; adjust it
for a portable or custom installation:

```powershell
$env:WGPU_BACKEND = "gl"
& "$env:ProgramFiles\VectorCraft\vectorcraft.exe"
Remove-Item Env:WGPU_BACKEND
```

The override is local to that PowerShell session. Remove it after the diagnostic run so the
next launch uses normal backend selection. Keep the startup log when reporting a driver issue.

For the same hardware on Linux, see the [Ubuntu graphics guide](ubuntu.md#graphics-hardware).
