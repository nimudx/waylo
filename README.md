# Waylo

A small desktop suite for **Hyprland** (Wayland), written in Rust. Everything draws on the CPU with
no GPU and no UI toolkit: the always-on pieces cost a few MB, and the heavy overlays run on demand
and leave nothing in memory when closed.

## Programs

| Binary | What it does | Lifetime |
|---|---|---|
| `waylo-bar` | Top bar, one per monitor: workspaces, clock, network, brightness, volume, battery, bluetooth. | Always on (~5 MB) |
| `waylo-watch` | Reapplies layout, workspaces and brightness when a monitor is connected or removed. | Always on (~1 MB) |
| `waylo-expose` | Window overview (F3) with live thumbnails and animations. | On demand |
| `waylo-launch` | Application launcher: compact icon grid, full-screen grid (`grid`) or list (`list`). | On demand |

## Crates

```
waylo-core    system access: Hyprland, monitors, workspaces, apps, battery, network, bluetooth
waylo-render  2D CPU drawing: shapes, text, icons (PNG/SVG), theme
waylo-shell   Wayland layer-shell surfaces, input, wl_shm buffers, animation frames
waylo-bar / waylo-expose / waylo-launch / waylo-watch   thin binaries wiring the above
```

Dependencies only point downwards: `core` and `render` depend on no other Waylo crate, `shell`
depends on `render`, and the binaries depend on the libraries.

## Build

```sh
cargo build --release
```

Binaries end up in `target/release/`. Runtime tools used: `hyprctl`, `grim` (thumbnails),
`pactl`/`wpctl` (volume), `nmcli` (network), `bluetoothctl` and `dbus-monitor` (bluetooth and power),
`fc-match` (fonts).

## Hyprland setup

Autostart `waylo-bar` and `waylo-watch`, and bind the on-demand programs, for example:

```lua
hl.bind("XF86LaunchA", hl.dsp.exec_cmd("pkill -x waylo-expose || waylo-expose"))             -- F3
hl.bind("SUPER + SPACE", hl.dsp.exec_cmd("pkill -x waylo-launch || waylo-launch"))           -- compact grid
hl.bind("code:248",      hl.dsp.exec_cmd("pkill -x waylo-launch || waylo-launch grid"))      -- full grid
```

For the frosted-glass look, add layer rules with blur for the `waylo-expose` and `waylo-launch`
namespaces.

## Status

Working: everything listed above. Missing: a config file for the bar, fractional scaling, and
windows/commands sections in the launcher.
