# Spike S5: arm64 Wine + FEX on the dev Mac (2026-11-09)

**Result: go.** Real x86 and x86_64 Windows code runs natively on Apple
silicon through Wine + FEX, with no Rosetta and no VM. D3D9 renders through
Metal.

## Setup

- **Engine:** mach9 v0.2.0 prebuilt bundle (`wine-v0.2.0-macos-arm64.tar.xz`,
  zlib). Wine 11.20-37 arm64/ARM64X, FEX, mtld3d, DXMT, vkd3d-proton,
  KosmicKrisp.
  - Downloaded with `gh release download`; the plain URL returned 504.
  - SHA-256 matched GitHub's digest (`243bcc45…a324`).
- **Loader `wine.app`:**
  - Signed "Developer ID Application: Alexander Theissen (WLKUUJD25W)".
  - Gatekeeper: "accepted, Notarized Developer ID". Hardened runtime, secure
    timestamp.
  - Entitlements: `cross-architecture-support`, `allow-jit`,
    `disable-executable-page-protection`, `disable-library-validation`,
    `custom-x18-abi-toggle`, `audio-input`.
  - Provisioning profile "Mach9 Loader": `ProvisionsAllDevices`, expires 2044.
  - Embedding apps must leave `wine.app` untouched. We therefore need no Apple
    developer account for the entitlement.
- **Paths:** the bundle is in `research/engines/mach9-v0.2.0/bundle/wine`; the
  test prefix is `research/engines/prefix-s5` (both git-ignored).
- **Environment:** `WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml="`.
- **Mac:** macOS 27 (Darwin 27), Apple silicon.

## Results

| Test | Code path | Result |
|---|---|---|
| `wine --version` | signed loader | `wine-11.20-37-g2362fb0442` |
| First run (prefix creation) | — | 16 s |
| `cmd /c ver` | native ARM64 | `Microsoft Windows 10.0.19045`, `arch=ARM64` |
| `syswow64\cmd.exe /c ver` (PE32, Intel 80386) | FEX via WoW64 | `arch=x86 host=ARM64`, exit 0 |
| `d3d9_test.exe stateblock`, i386 | FEX WoW64 + mtld3d | 14,738 checks, 0 failures, about 1 s |
| `d3d9_test.exe stateblock`, x86_64 | ARM64EC + FEX + mtld3d | 14,738 checks, 0 failures, about 1 s |
| `d3d9_test.exe visual`, x86_64, **mtld3d** | D3D9 → Metal | 211,249 checks, **895 failures** (0.42%), 22 skipped, **21 s** |
| `d3d9_test.exe visual`, x86_64, **wined3d** | D3D9 → OpenGL → Zink → Vulkan → KosmicKrisp → Metal | 211,540 checks, **8 failures**, 8 skipped, 50 s |

wined3d was selected with `WINE_COMPATDB='exe=d3d9_test.exe;d3d9=wined3d'`.

mtld3d's failures cluster in a few feature areas:

| Area | Lines of `visual.c` | Failures |
|---|---|---|
| Fixed-function lighting | 683–713 | 7 |
| Unidentified, possibly fog (not checked) | 1138 | 108 |
| Unidentified | 15635 | 128 |
| Unidentified | 15745 | 32 |
| Depth formats D16/D24S8/D24X8/D32 | 16097 | 24 each |
| Pixel shaders | 27793 | 25 |

## Takeaways

- **Backend trade-off:** mtld3d is about 2.4× faster on this suite but misses
  some features; wined3d is nearly conformant but slower. A per-game rule picks
  between them, and that rule is crossfree's job.
- **Test time is not game frame rate.** Real games must be measured before
  choosing defaults.
- **Where to contribute:** fixing mtld3d's failing feature areas upstream is
  higher-leverage than a separate D3D9 layer.
- **Gatekeeper:** a GitHub-distributed crossfree app without a Developer ID
  shows a Gatekeeper prompt on first launch. The embedded `wine.app` keeps its
  own notarized signature.
