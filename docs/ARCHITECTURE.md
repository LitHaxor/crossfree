# crossfree — architecture plan (v0, 2026-11-09)

> **Direction (2026-11-09): integrate first.** crossfree assembles proven
> open-source layers instead of writing its own translators. Its own code is
> what users touch: the app, Steam integration, per-game rules and the
> compatibility database.
> - **CPU:** native arm64 Wine + FEX, using public macOS 26 APIs (§3.1).
>   Rosetta is only a fallback for macOS releases older than 26.4.
> - **Graphics:** DXVK (D3D9–11, via Vulkan on KosmicKrisp or MoltenVK), DXMT
>   (D3D10/11 directly on Metal), vkd3d-proton (D3D12).
> - **Custom D3D9 layer (§2–§3.2): parked.** A mature D3D9 layer is 24k–80k+
>   lines (DXVK's d3d9 is about 24k, DXMT about 81k), so crossfree writes
>   graphics code only where measurements show a real gap, and contributes it
>   upstream (mtld3d, DXMT) where possible. `crates/d3d9-bytecode` and
>   `crates/shader-ir` are tested and kept; `crates/msl-emitter` has golden MSL
>   tests, and every Wine corpus shader it emits compiles in Metal
>   (`cargo test -p msl-emitter --test metal_compile -- --ignored`).
> - **Done:** spike S5 (`docs/spikes/s5-arm64-wine-fex.md`). The launcher
>   core (`crates/launcher`, CLI `crossfree`) manages prefixes and launches
>   programs with per-game rules on mach9's engine.
> - **Done since:** the SwiftUI app (a console-style launcher: Games,
>   Directory of stores, Settings), the engine shipped inside the app,
>   Windows Steam in a prefix, per-game Dock names (`crates/launcher/src/dock.rs`)
>   and running-game detection.
> - **Next:** storing profiles (SQLite) and the compatibility database.
> - Details: `research/README.md`, "Update".

crossfree runs Windows games natively on Apple silicon:
- no VM;
- no closed or cracked components;
- only open-source parts, under their own licenses.

This document covers the overall shape. Sections 2–3.2 describe the parked
custom D3D9 layer: a Direct3D 9 / 9Ex → Metal translator with a shader JIT.

Evidence and sources are in [`../research/README.md`](../research/README.md).
Items marked *(unverified)* are from memory and must be checked before relying
on them.

---

## 1. The stack

```
 SwiftUI app ──► launcher (Rust) ──► Wine (crossfree fork of CodeWeavers' LGPL Wine)
  library,        bottles, per-game           │
  profiles        rules (WINE_COMPATDB;       ├─ CPU: x86/x86-64 code ─► FEX (JIT) in native arm64 Wine
                  SQLite later)               │                          (Rosetta fallback, see §3.1)
                                              ├─ D3D9 / 9Ex ─► DXVK ─► Vulkan (KosmicKrisp/MoltenVK) ─► Metal
                                              │                (custom D3D9 → Metal layer: parked, §2)
                                              ├─ D3D10/11   ─► DXMT (LGPL) ─► Metal, or DXVK
                                              ├─ D3D12      ─► vkd3d-proton ─► KosmicKrisp/MoltenVK ─► Metal
                                              └─ D3DX9      ─► Wine builtin d3dx9_* (sits on top of d3d9)
```

**Why D3D9 first.** The mainstream Mac tools leave D3D9 on indirect paths:
- CrossOver's default uses `wined3d`, which goes D3D9 → Apple's deprecated OpenGL.
- CrossOver's DXVK option goes D3D9 → Vulkan → MoltenVK.
- UTM goes D3D9 → D3D9On12 → D3DMetal.
- DXMT and D3DMetal have no D3D9 support.

Only the young mtld3d project (zlib, started 2026) goes D3D9 → Metal directly,
with the same overall shape as crossfree's D3D9 layer. crossfree differentiates on the shader
JIT (§3.2) and studies mtld3d's documented trade-offs (`docs/STATUS.md` in its
repo) as prior art.

**Base Wine.** Build an arm64 (ARM64X) Wine that uses the macOS 26 APIs: 4 KB
pages, per-thread hardware TSO, and the custom x18 ABI. Starting points:
- Upstream Wine plus Brendan Shanks' (CodeWeavers) patches as they land.
- CodeWeavers' LGPL CrossOver source, for MSync and macOS fixes.

Because crossfree builds its own Wine, it can add small, clean hooks instead of
reaching into Wine's internals (see §4.4).

---

## 2. D3D9 layer design (parked)

### 2.1 Process layout
A Wine graphics DLL has two halves in the same process. The PE half is Windows
code that games call; the "unix" half is native macOS code. They talk through
`__wine_unix_call`. The D3D9 layer keeps the PE half thin and puts the real work in
native Rust:

```
 game.exe (x86/x86-64, emulated by FEX)
   │  COM calls: SetRenderState, SetTexture, DrawIndexedPrimitive, Lock, Present …
   ▼
 d3d9.dll   [PE, Rust, Wine builtin: i686 (emulated by FEX) and ARM64X (native for 64-bit games)]
   - COM objects (IDirect3D9(Ex), Device9(Ex), resources, stateblocks)
   - API validation; answers Get* calls from shadowed state, without calling the core
   - encodes commands into a ring buffer (no per-call unix transition)
   │  __wine_unix_call: only on flush / sync points / object creation
   ▼
 d3d9.so [unix, Rust, aarch64-apple-darwin; x86_64 only for the Rosetta fallback]
   - command-stream consumer on its own thread (like DXVK's CS thread)
   - state → Metal: pipeline keys, argument buffers, render/blit encoders
   - shader JIT + pipeline cache (§3.2)
   - swapchain: HWND → CAMetalLayer, frame pacing
   ▼
 Metal (system framework) ─► Apple GPU
```

**Why not copy DXMT's layout?** DXMT puts its logic in the PE half and uses a
thin bridge that makes one unix call per Metal call. The D3D9 layer does the opposite:
- **Fewer crossings.** D3D9 games make huge numbers of tiny calls. A thin PE
  half that batches them crosses into native code once per flush instead of once
  per call.
- **Idiomatic Rust.** The core uses `objc2-metal` natively instead of thunked
  handles.
- **Testable without Wine.** The core is an `rlib`, so `cargo test` can drive it
  on macOS with offscreen render targets.
- **Same core for any CPU layer.** It recompiles for aarch64 unchanged.

### 2.2 Memory and resources
Apple silicon has unified memory: the CPU and GPU share it, so data does not
need copying.
- **Sharing the ring buffer.** Under WoW64, the PE and unix halves share one
  address space. The PE half allocates the ring and buffer storage below 4 GB
  with `NtAllocateVirtualMemory`, so 32-bit pointers stay valid on the 64-bit
  side.
- **Buffers.** Vertex and index buffers are `MTLStorageModeShared`, so `Lock`
  returns a pointer straight into GPU-visible memory.
  - `D3DLOCK_DISCARD` swaps in a fresh suballocation, retired by a GPU fence.
  - `D3DLOCK_NOOVERWRITE` returns the current one.
- **Textures.** `DEFAULT` textures are private, and uploads go through staging
  blits. `MANAGED` textures keep a CPU copy and upload on unlock or first use.
  `SYSTEMMEM` textures are CPU-only.
- **Sync points.** These force a flush plus a fence wait:
  - readbacks: `GetRenderTargetData`, `LockRect` on a render target;
  - query results;
  - `Present` with `D3DPRESENT_DONOTWAIT`.

### 2.3 D3D9 → Metal mapping (needs emulation)
The reference is `research/sources/dxvk-upstream/src/d3d9`.

| D3D9 feature | Metal approach |
|---|---|
| Fixed-function vertex/pixel pipeline | Generate shader IR from a fixed-function state key (§3.2). |
| Alpha test | `discard_fragment` controlled by a function constant. |
| Triangle fans | Rewrite the index buffer, cached per (fan buffer, range). |
| `D3DCOLOR` vertex data | Native `uchar4Normalized_bgra`. |
| Half-pixel offset, pixel centers at 0,0 | Vertex-shader epilogue adjustment. |
| POSITIONT (pre-transformed vertices) | Pass-through vertex shader with viewport un-transform. |
| `SRGBWRITEENABLE`, sRGB textures | sRGB/linear pixel-format views. |
| INTZ / DF16 / DF24 / Fetch4 | Depth textures readable as color, with a gather path. Check D24S8 with `isDepth24Stencil8PixelFormatSupported`. |
| Software vertex processing (SWVP) | Compute or CPU path; late milestone. |
| Clip planes | Metal clip distances (`[[clip_distance]]`). |
| Fog, point sprites | Generated in the shader epilogue. |
| Lost devices, `Reset`, D3D9Ex differences | Handled in the PE half's state machine. |

---

## 3. JIT strategy — "translate to ARM64 and Metal all the time"

There are two separate translators: CPU instructions and GPU shaders. They solve
different problems and share no internal representation. The goal for both is
the same:
- translate the first time code is seen;
- cache the result;
- **never translate the same thing twice**, so the steady state runs at full
  speed.

### 3.1 CPU: x86/x86-64 → ARM64
Rosetta 2 is fully available only through macOS 27. macOS 28 keeps a
games-only subset. So crossfree runs Wine natively on arm64 and lets **FEX**
translate x86 code just in time, through Wine's emulator-DLL interface:
- **32-bit games:** WoW64, using `libwow64fex.dll`.
- **64-bit games:** ARM64EC, using `libarm64ecfex.dll`. Wine and the graphics
  stack are ARM64X and run natively; only the game's own code is emulated.

macOS 26 provides public APIs for every former blocker. They are verified in
the MacOSX 27.0 SDK headers on the dev Mac (see research README):

| Problem | API | Notes |
|---|---|---|
| Windows assumes 4 KB pages; arm64 macOS uses 16 KB | `posix_spawnattr_set_4k_page_size_np()` (`spawn.h`, macOS 26.0+) | Restricted entitlement `com.apple.developer.cross-architecture-support`, with a provisioning profile embedded in the loader bundle. Others ship it in Developer ID notarized apps. |
| x86 memory ordering (TSO) | `thread_set_x86_64_compat(1)` (`mach/mach_traps.h`) | Per thread; new threads do not inherit it, so set it in every Wine thread start. |
| Windows ARM64 uses x18 for the TEB | `os_set_custom_x18_abi_enabled()` (`os/arch/arm64.h`) | Switch on and off at the boundary with system frameworks; the application saves and restores x18 itself. |
| USER_SHARED_DATA at 0x7ffe0000 is inside PAGEZERO | `-Wl,-x86_64_layout_emulation` (Xcode 26.4 linker) | Shrinks PAGEZERO. Per wine-devel, not verified locally. |
| JIT code under hardened runtime | `MAP_JIT`, `pthread_jit_write_protect_np`, `com.apple.security.cs.allow-jit` | Fine for notarized Developer ID apps; not for the Mac App Store *(unverified)*. |

- **Rosetta fallback:** for macOS releases older than 26.4 only, build an
  x86_64 Wine and run it under Rosetta, as CrossOver does.
- **x87 floating point:** D3D9-era games use x87 math heavily. Emulating it
  exactly is slow. FEX has a reduced-precision x87 mode; mach9 enables it by
  default. Measure it per game.
- **Spike S5:** bring up arm64 Wine + FEX on this Mac with the APIs above, then
  run one 32-bit and one 64-bit test program.

### 3.2 GPU: D3D9 shader bytecode → Metal
D3D9 shaders arrive at runtime as SM1–3 bytecode, so this translator is
inherently a JIT. It has tiers so the game never stalls waiting for it:

```
 d3dbc bytecode ─► d3d9-bytecode (parse, validate) ─► shader-ir (SSA) ─┬─► MSL text ─► MTLLibrary (Metal compiles)  tier 1
 fixed-function state key ─► fixed-function ─────────────────────┘└─► AIR bitcode ─► MTLLibrary (no MSL)       tier 2
                                                     
 pipeline key = shaders + vertex layout + blend/depth formats + sample count
 D3D9 per-draw toggles = Metal function constants
 (alpha func, fog mode, sRGB write, clip-plane mask, half-pixel, point sprites)
```

- **Tier 0 — cache hit.** Look up the pipeline key in crossfree's disk cache
  (AIR/MSL plus an `MTLBinaryArchive` of compiled pipelines), under
  `DARWIN_USER_CACHE_DIR/crossfree/<exe-hash>/`. Most frames should only hit
  this tier.
- **Tier 1 — MSL.** Emit Metal Shading Language and compile it asynchronously
  on a thread pool. Simple and debuggable: the MSL can be dumped and read.
- **Tier 2 — AIR.** Emit AIR (Metal's LLVM-based internal format) directly and
  load it with `newLibraryWithData`, skipping Metal's source frontend. This is
  the DXMT `airconv` approach (LGPL; see
  `research/sources/dxmt/src/airconv`).
  - AIR is **undocumented by Apple** and may change between macOS releases.
    Tier 1 therefore always stays as the fallback.
  - A crossfree self-test validates tier 2 against tier 1 on first run after an
    OS update.
- **Avoiding hitches.**
  - While a specialized pipeline compiles, draw with a precompiled
    **fixed-function ubershader** (the Dolphin-emulator technique) or skip the
    draw for one frame, configurable per game.
  - On launch, warm up from pipeline keys recorded in earlier sessions, like
    DXVK's state cache.
- **Correctness oracle.** For every shader in the test corpus, compare crossfree's
  output with vkd3d-shader (d3dbc → SPIR-V) → SPIRV-Cross → MSL rendered on the
  same inputs.
  - vkd3d-shader 1.18 cannot output MSL from d3dbc; the vkd3d 2.1 changelog
    adds none either. That is why it serves only as an oracle, not a dependency.
- **Apple's Metal Shader Converter** accepts DXIL (SM6+) only, so it does not
  apply to D3D9.

---

## 4. Crates and repo layout

```
crossfree/
  Cargo.toml                 # workspace
  crates/
    d3d9-bytecode/       # done: SM1–3 bytecode decoder + disassembler. Pure Rust, no unsafe.
    shader-ir/           # shader IR (SSA, typed), passes (lowering, dead-code elimination, constant folding)
    fixed-function/      # fixed-function state key → shader-ir
    msl-emitter/         # shader-ir → MSL text (tier 1)
    air-emitter/         # shader-ir → AIR bitcode + metallib container (tier 2, later)
    command-protocol/    # #[repr(C)] command-stream format shared by PE and unix halves
    d3d9-runtime/        # unix half: Metal backend (objc2-metal), pipeline cache, swapchain. rlib + cdylib (d3d9.so)
    d3d9-dll/            # PE half: COM objects, state shadowing, encoder. cdylib d3d9.dll
    launcher/            # later: bottles, rules engine, SQLite profiles
  app/                   # later: SwiftUI front end
  wine/                  # build scripts + patch queue for crossfree's arm64 Wine
  docs/                  # this file, ADRs
  research/              # git-ignored: notes and third-party sources
```

### 4.1 Toolchain
- **PE half:** Rust, linked with llvm-mingw, built with `panic = "abort"`.
  - `i686-pc-windows-gnu` for 32-bit games. Under FEX this code is emulated,
    so it has to stay thin.
  - ARM64X for 64-bit games, so the half runs natively next to emulated game
    code. Rust's `arm64ec-pc-windows-msvc` target is tier 3; spike S2 decides
    the toolchain.
  - `x86_64-pc-windows-gnu` only for the Rosetta fallback.
  - After linking, run `winebuild --builtin` to mark the DLL as a Wine builtin;
    DXMT does the same in `src/winemetal/meson.build`. Wine only hands unix-call
    tables to builtin DLLs.
- **Unix half:** Rust `aarch64-apple-darwin` (`x86_64-apple-darwin` for the
  Rosetta fallback).
  - Exports `__wine_unix_call_funcs` and `__wine_unix_call_wow64_funcs`. The
    WoW64 table converts the 32-bit parameter structs, as DXMT's
    `winemetal_unix.c` does.
- **COM interfaces in Rust:** use `#[repr(C)]` vtables, either hand-written or
  through `windows-core`. Spike S2 decides which.

### 4.2 Testing and quality gates
Every change ships with tests, and CI must be green before merging.

| Layer | Tests | Gate |
|---|---|---|
| d3d9-bytecode / shader-ir / msl-emitter / fixed-function | unit tests; golden MSL snapshots; `cargo-fuzz` on the parser | coverage ≥ 85% (`cargo-llvm-cov`); no fuzz crashes |
| d3d9-runtime | native Metal tests rendering offscreen, compared against reference images | coverage ≥ 70% |
| Translator correctness | corpus = every shader embedded in Wine's d3d9 tests, checked against the vkd3d/SPIRV-Cross oracle | no unexplained diffs |
| Whole DLL under Wine | Wine's own conformance tests: `dlls/d3d9/tests` (`device.c`, `visual.c`, `stateblock.c`, `d3d9ex.c`, about 52k lines) | pass rate tracked per commit; target ≥ wined3d baseline |
| Games | smoke list of D3D9 titles the developer owns | manual per milestone |

Native Metal tests need a real GPU. GitHub-hosted macOS runners may not provide
one *(unverified)*, so plan for a self-hosted Apple-silicon runner.

### 4.3 Licensing
- **Wine and DXMT code:** stays LGPL-2.1. Anything copied from them stays in
  LGPL files and is dynamically linked.
- **crossfree's own code:** MIT (decision D4, §6). Anything taken from Wine
  or DXMT keeps LGPL-2.1 in files of its own.
- **Never ship:** D3DMetal, Metal Shader Converter (terms unverified), or
  anything from CrossOver's closed parts.

### 4.4 Wine integration points (crossfree Wine patch queue)
1. **HWND → Metal view.** Export
   `macdrv_view_create_metal_view` / `macdrv_view_get_metal_layer` lookups
   by HWND as a stable function in crossfree's Wine.
   - DXMT instead copies winemac's private `macdrv_win_data` struct layout,
     which breaks whenever Wine changes it.
2. **Backend selection.** `launcher` does not set `WINEDLLOVERRIDES`. It
   removes the inherited variables that conflict with the engine's launcher
   contract (`compatdb_core::inherited_conflicts()`, which includes
   `WINEDLLOVERRIDES`), then passes per-program rules, such as the D3D9 and
   DXGI backend, in `WINE_COMPATDB`. `compatdb-core`, pinned to the engine's
   release, writes those rules, and the engine applies them per process. No
   closed module is involved; CrossOver uses its `prepend_dll_path` export.
3. **Fake vendor DLLs** (`nvapi`/`atidxx`) only where a game needs them; not
   needed for D3D9.

---

## 5. Milestones

| Milestone | Scope | Exit criteria |
|---|---|---|
| **Phase 0 — spikes** | S1 build CrossOver-26.3 Wine (x86_64) on this Mac and record the wined3d baseline on Wine's d3d9 tests. S2 Rust PE `d3d9.dll` loads as a builtin in 32- and 64-bit with a unix-call round trip, including WoW64. S3 Rust core clears a Wine window via `CAMetalLayer`. S4 d3d9-bytecode parses all test-corpus shaders and msl-emitter output compiles in Metal. S5 bring up native arm64 Wine + FEX with the macOS 26 APIs (§3.1). **S4 status: d3d9-bytecode decoder done**: all 197 shaders in Wine's d3d9 tests handled (195 decode; 2 are rejected exactly as Wine expects). | Each spike: a short write-up in `docs/spikes/` and a go/no-go decision. |
| **M1 — first triangle** | Device and swapchain, `Clear`, `Present`, vertex/index buffers, SM2/3 programmable shaders via tier 1, basic render states | A spinning-cube sample runs; Wine `device.c` subset passes |
| **M2 — fixed function and textures** | fixed-function, textures and samplers, alpha test, fog, stateblocks, managed pool, D3D9Ex | `visual.c` pass rate ≥ 50% |
| **M3 — real games** | Queries, MSAA, INTZ/DF24, triangle fans, POSITIONT, sRGB, disk pipeline cache, async compile, ubershader fallback | 3 owned D3D9 titles playable; pass rate ≥ wined3d baseline |
| **M4 — JIT performance** | air-emitter (tier 2), `MTLBinaryArchive`, pipeline warm-up | Shader hitches eliminated on repeat runs; CPU time per draw measured below wined3d |
| **Later** | launcher + rules DB, SwiftUI app, DXMT for D3D10/11, D3D12 path, CPU-layer plan B (from S5) | — |

## 6. Decisions for the owner

| # | Decision | Recommendation |
|---|---|---|
| D1 | Base Wine | arm64/ARM64X Wine (upstream plus the macOS 26 API patches as they land), taking MSync and macOS fixes from CodeWeavers' LGPL source |
| D2 | Where the D3D9 layer does its work | Thin PE half + native Rust core (§2.1) |
| D3 | Tier-1 shader path | Own Rust d3dbc → IR → MSL, with vkd3d/SPIRV-Cross as the test oracle |
| D4 | License for crossfree's own crates | **Decided (2026-11-10):** MIT for all of crossfree's own code; code taken from Wine or DXMT keeps LGPL-2.1 in files of its own |
| D5 | CPU layer | **Decided:** FEX through Wine's emulator-DLL interface, with hardware TSO via `thread_set_x86_64_compat`; Rosetta only as a fallback for older macOS |

## 7. Risks

- **Restricted entitlement.** 4 KB pages need
  `com.apple.developer.cross-architecture-support`, which requires an Apple
  developer account and a provisioning profile, and Apple could change its
  terms. Mitigation: Rosetta fallback on older macOS; follow how mach9 and
  MetalSharp ship it.
- **FEX on macOS is young.** Mitigation: contribute fixes upstream (FEX is MIT)
  rather than fork.
- **AIR format drift** (undocumented). Mitigation: keep tier 1 MSL as the
  fallback, with a self-test after OS updates.
- **WoW64 32-bit thunk correctness.** Most D3D9 games are 32-bit. Mitigation:
  test the 32-bit path from S2 onwards.
- **Scope.** D3D9 has a long tail of behaviour. Mitigation: measure everything
  against Wine's conformance tests, and use DXVK's d3d9 as the behavioural
  reference.
