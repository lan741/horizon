# Horizon on iPadOS

The port's staging plan, what this branch already does, and the platform rules that shape the rest.

**Status: M0.** `crates/horizon-ios` builds, runs a Metal surface, reads touches and reports its
frame rate; the CI workflow turns it into an `.ipa`. The engine itself is not linked yet — M1 and
M2 below are the work that connects it, and this file names every seam they touch.

## What this branch adds

| Path | What it is |
| --- | --- |
| `crates/horizon-ios/` | The app bundle's executable: entry, Metal surface, touch bridge, container paths, a file logger. 13 unit tests, no device needed. |
| `packaging/ios/Info.plist` | iPad-first bundle: arm64 + Metal, iOS 15 floor, four orientations, full screen, `CADisableMinimumFrameDurationOnPhone`. |
| `packaging/ios/make-ipa.sh` | Assembles `Payload/Horizon.app` around the executable, signs it, zips the `.ipa`. No Xcode project. |
| `packaging/ios/AppIcon/` | The icon set iOS reads from the bundle root. |
| `.github/workflows/ios-ipa.yml` | macOS runner: host tests, `aarch64-apple-ios` build, bundle, `.ipa` artifact. |
| `docs/ios-port.md` | This file. |

## Why this is a port and not a conversion

There is no step that turns the Windows build into an iPad one. The client is a winit + wgpu
program; the same source builds for iOS once the platform differences below are answered. The
engine already carries parts of that answer: `crates/horizon-client-render/src/touch.rs` is a
complete on-screen control system (`TouchControls` → `PadState`, written for Android first),
`android_runtime.rs`/`keypad.rs` arbitrate which input device is in charge, `horizon-threads`
reads RSS on iOS through `task_info`, and `horizon-dualsense` builds with a haptics stub there.
What does not exist yet is the entry that starts the engine inside an app bundle, and the
renderer's device on a Metal-only system.

Three platform rules decide the shape:

1. **No `fork`/`exec`.** `crates/horizon-client/src/entry.rs` starts by running
   `std::process::Command::new(current_exe())` to hand the command line to
   `LaunchArguments::for_command`. iOS refuses to spawn. The port builds `LaunchArguments`
   in-process (M1).
2. **No JIT.** `crates/horizon-mod-wasm` compiles WebAssembly with wasmtime + Cranelift. iOS
   forbids writable-executable memory, so mods run on wasmtime's **Pulley** interpreter instead
   (the feature is already enabled in that crate). Slower, and it keeps content mods working.
3. **No `dlopen` of installed libraries.** Native (dylib) mods cannot load on iOS at all; they are
   refused with a message rather than half-installed (M5).

Two smaller ones: the app writes only inside its container (`Documents`, `Library/Caches`, `tmp`
— `crates/horizon-ios/src/paths.rs`), and the process is suspended, not closed, when the player
leaves (`applicationWillResignActive` → winit's `suspended`), so the game must save there.

## 120 Hz and the interaction model

The panel runs at 120 Hz; what an app gets is decided by three things, all handled here:

- **Present mode.** `wgpu::PresentMode::AutoVsync` in `crates/horizon-ios/src/gpu.rs`, so a frame
  is presented once per display refresh and iOS drops the rate when nothing changes.
- **The plist.** `CADisableMinimumFrameDurationOnPhone` is set; without it UIKit holds the app to
  60 Hz. `UIRequiresFullScreen` keeps Stage Manager from resizing the view under a frame.
- **Measurement, not hope.** The app requests one redraw per callback and logs the measured rate
  every two seconds (`Horizon iOS: 119.8 frames/s`) to `Documents/Horizon/horizon-ios.log`. A
  device that reports 60 there is a device that is not at 120, and the log says so.

Touches (`crates/horizon-ios/src/touch.rs`): the left 42% of the window is the thumb stick, the
rest looks. The stick has a 15% dead zone and a 12% travel radius and re-scales past the dead zone
so the first millimetre of travel is not lost; a touch that ends within 180 ms and 2% drift is a
tap (jump), anything longer is a hold the game maps to mining. Positions are normalized once, at
the platform edge, so the engine's `TouchControls` receives the same units on every screen size.
That module is also where the safe-area inset belongs (M3): the surface is the whole window, the
HUD is not.

## Stages

| | What lands | Done when |
| --- | --- | --- |
| **M0** | Platform shell, packaging, CI, this file | The `.ipa` installs; the log names the Metal device, the swapchain and the frame rate. **Done in this branch.** |
| **M1** | The engine in-process: `LaunchArguments` without a spawn, `HORIZON_DATA_DIR` at the container, `entry::start` from the app | The client starts, prepares its files and ticks a world with no window |
| **M2** | The renderer on Metal: `open_device` with `Backends::METAL`, the surface from the app's window, HDR output off, `max_texture_dimension_2d` respected | A world renders; the frame rate matches the panel |
| **M3** | Input and UI: winit `Touch` → `TouchControls`, safe-area insets, the on-screen keyboard (`osk.rs`), `GCController` for pads | A player walks, looks, mines, places and opens the inventory with fingers only |
| **M4** | Storage: Files-app import of a `.minecraft` folder, security-scoped URLs copied into `Documents/Horizon` | A world imported on the iPad opens and saves |
| **M5** | Mods and network: wasm mods on Pulley, native mods refused, LAN discovery allowed | A content mod loads; a LAN world is joined from the iPad |

M1 is the only stage that edits engine files: `entry.rs` gains an iOS constructor for
`LaunchArguments` and the client's `Options::read` runs against the container paths. Everything
else is additive — a platform crate plus wiring — which is why the port does not fork the game.

## Limits to accept

- **Mods are interpreted.** With Pulley, a heavy mod is slower than on desktop; the mod screen
  should say so on iOS rather than fail silently.
- **Native mods do not load.** No workaround exists on a signed app.
- **The process can be killed in the background.** The platform layer logs `suspended` so the
  engine can save; a kill after that is normal on iOS.
- **Memory is a device limit.** The renderer's distance and pack options should default lower on
  an iPad than on a desktop; `resident_bytes` already reports what the process holds.
- **Splitscreen, HDR output and the macOS menu-bar window code are inert.** They are desktop
  features, not regressions.

## Build and install

```bash
# On macOS with Xcode: the same commands the workflow runs.
rustup target add aarch64-apple-ios
cargo test -p horizon-ios --lib
cargo build --release --target aarch64-apple-ios -p horizon-ios
BIN=target/aarch64-apple-ios/release/horizon-ios VERSION=0.1.0 BUILD=1 \
  packaging/ios/make-ipa.sh
# → dist/ios/Horizon-0.1.0.ipa
```

The workflow (`iOS IPA`) does that on GitHub's macOS runner and uploads the `.ipa` as an artifact.

Install it with **TrollStore** (unsigned, permanent on supported versions) or re-sign it with
**Sideloadly / AltStore / ESign** and an Apple ID or certificate. Ad-hoc signed builds
(`SIGN_IDENTITY=-`) are what the workflow produces by default.

## On-device checklist

1. The app opens to a gradient with the stick ring at the left.
2. `Documents/Horizon/horizon-ios.log` names the GPU, the swapchain and, after two seconds, the
   frame rate: `~120.0` on a ProMotion iPad, `60.0` elsewhere.
3. A thumb on the left moves the ring's knob; a drag on the right moves the reticle.
4. Rotating the iPad keeps the picture upright and the log shows a new surface size.
5. Home then back: the log shows `suspended` and a fresh `resumed`, with the surface reconfigured.