# ratfetch

[![ci](https://github.com/tuna4ll/ratfetch/actions/workflows/ci.yml/badge.svg)](https://github.com/tuna4ll/ratfetch/actions/workflows/ci.yml)

A system fetch that does not stop at one frame.

`fastfetch` prints your machine once and exits. `ratfetch` keeps the logo and
the info table, then puts them at the top of a live TUI: meters, graphs, a
process table, filesystems and interfaces, all refreshing while you watch.

<img width="1920" height="1080" alt="image" src="https://github.com/user-attachments/assets/98353701-4342-480c-99fd-4c96b5d0f2d4" />

## What it does

- **Live, not a snapshot.** Everything re-reads on a configurable interval.
- **526 logos**, the full set vendored from fastfetch, with their original
  colour palettes. Auto-detected from `/etc/os-release`, or pick your own.
- **Four tabs** — overview, processes, filesystems, interfaces.
- **13 built-in themes**, plus per-role colour overrides. The default is
  `mono`, so the interface borrows your terminal's colours and the logo is the
  only thing on screen with a palette of its own.
- **A config system that tells you when you get it wrong**, with the file, the
  line, and a "did you mean" for every closed vocabulary in the schema.
- **No `sysinfo` crate.** `/proc` and `/sys` are read directly, so the numbers
  come from where the kernel actually puts them.

Linux only — the collectors are built on `/proc` and `/sys`.

## Install

```sh
git clone https://github.com/tuna4ll/ratfetch
cd ratfetch
cargo install --path .
```

Needs Rust 1.88 or newer.

## Use

```sh
ratfetch                      # the live TUI
ratfetch --once               # one frame, like a classic fetch tool
                              # (graphs are dropped: one sample is not a series)
ratfetch --logo nixos         # override the detected logo
ratfetch --theme gruvbox-dark
ratfetch --interval 250       # four samples a second
ratfetch --list-logos | fzf   # browse what is bundled
```

### Keys

| key | action |
|-----|--------|
| `q` `esc` `ctrl+c` | quit |
| `?` `f1` | help |
| `tab` `shift+tab` | change tab |
| `j` `k` / arrows | scroll |
| `s` | cycle the process sort column |
| `c` | per-core CPU meters |
| `space` | freeze / resume |
| `r` | reload the config |

All of them are rebindable.

## Configuration

```sh
ratfetch --generate-config     # writes a fully commented ~/.config/ratfetch/config.toml
ratfetch --check-config        # validates without starting the UI
ratfetch --print-config        # the effective config, after merging
```

Files are merged in order, and each one may be partial:

```
/etc/ratfetch/config.toml  →  ~/.config/ratfetch/config.toml  →  --set key=value
```

`--config PATH` replaces the first two. `--no-config` skips them all.

The config is watched while running, so saving the file re-applies it. If the
new version does not parse, ratfetch says so in the footer and keeps running on
the old one.

### A taste of it

```toml
[general]
interval_ms = 500

[logo]
source = "arch"
color_mode = "rainbow"

[theme]
name = "tokyonight"

[theme.colors]
accent = "#7aa2f7"

[layout]
panels = ["meters", "graphs", "processes"]

[info]
items = ["title", "separator", "os", "kernel", "uptime", "cpu", "memory", "colors"]

[meters]
style = "blocks"
per_core = true

[keys]
quit = ["q"]
freeze = ["p"]
```

Anything you leave out keeps its default. Anything you misspell is an error
with a location:

```
$ ratfetch --check-config
ratfetch: in /home/tuna4l/.config/ratfetch/config.toml:
TOML parse error at line 12, column 1
   |
12 | inteval_ms = 500
   | ^^^^^^^^^^
unknown field `inteval_ms`, expected one of `interval_ms`, `history`, ...
```

```
$ ratfetch --set info.items='["kernal"]' --check-config
ratfetch: config is not valid: unknown InfoItem `kernal` (did you mean `kernel`?)
```

Every option is documented inline in the generated config; that file is the
reference.

## Layout

The overview is a header plus a stack of panels:

- `layout.panels` — `meters`, `graphs`, `processes`, `disks`, `network`, `colors`
- `layout.panel_heights` — a fixed height each, or `0` to share what is left
  (`meters` sizes itself to its bars)
- `logo.position` — `left`, `right`, `top`, `none`

Below `layout.narrow_width` columns the logo moves above the info table on its
own, so ratfetch stays readable in a split pane.

## Credits

The ASCII art is vendored from [fastfetch](https://github.com/fastfetch-cli/fastfetch)
under the MIT License — see `assets/ascii/LICENSE`. The TUI is built with
[ratatui](https://ratatui.rs), which the name is a nod to.

## License

MIT © Tuna Kılıç
