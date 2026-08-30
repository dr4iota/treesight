# treeserve

A single-binary file server in the spirit of `webfsd`, but with server-side
rendering: directory browsing with a file tree side pane, syntax-highlighted
code, rendered Markdown, inline image/video/audio/PDF preview, and dark/light
themes. Plain HTML output, zero JavaScript, no runtime dependencies.

## Build

```sh
./build.sh            # or build.bat on Windows → dist/treeserve, dist/treesight
./build.sh server     # just the server, no webview libraries needed
./build.sh static     # server as one fully static file (musl, no libc)
./build.sh windows    # both .exe files, cross-compiled → dist/windows/
./build.sh install ~/bin       # copy dist/* there; ~/bin is the default
./build.sh bundle     # also the installers (needs cargo-tauri, see below)
```

On Windows the destination is required rather than defaulted, so nothing can
land somewhere you did not ask for:

```bat
build.bat install C:\WinApps
```

Both scripts print what they produced and warn when the destination is not on
your `PATH`.

How the two crates share a server, how a request becomes a page, and where
a downstream app may plug in: [docs/architecture.md](docs/architecture.md).

Two crates, two binaries:

| | |
|---|---|
| `treeserve` | the HTTP server and its CLI — ~8 MB, no system dependencies |
| `treesight` | the desktop app: the same server in a native window |

Plain `cargo build` at the root builds only `treeserve`, so the desktop app's
webview libraries (`libwebkit2gtk-4.1-dev` on Linux, WebView2 on Windows,
WKWebView on macOS) are needed only when you ask for `treesight`.

**Both binaries run fully offline.** Stylesheets, all ~30 syntax themes, the
Markdown parser, the Mermaid→SVG renderer and the LaTeX→MathML renderer are
compiled in; a served page
references no external host, so nothing is fetched at run time — no CDN, no
webfonts, no telemetry. The only network access is at build time: `cargo`
fetching crates, and, for `build.sh bundle`, the Tauri bundler fetching its
NSIS/AppImage tooling and the WebView2 runtime (see below).

## Portable binaries

Both binaries are single files with everything compiled in — no data directory,
no config file, nothing fetched at run time. How portable each one is depends
only on what it links against:

| | portability |
|---|---|
| `treeserve`, default build | one file, links the system libc; runs on the same or newer glibc |
| `treeserve`, `./build.sh static` | one file, **no libc at all** — copy it to any Linux of the same architecture |
| `treeserve.exe`, `build.bat` | one file; needs the Visual C++ runtime unless you uncomment the `crt-static` line |
| `treeserve.exe`, `./build.sh windows` | one file; imports only OS DLLs and the Universal CRT, so there is no runtime to install |
| `treesight.exe` | uses the WebView2 runtime that ships with Windows 11 and current Windows 10. One file from `build.bat`; cross-compiled it also needs `WebView2Loader.dll` beside it |
| `treesight` (macOS) | one file; WKWebView is part of the OS. A bare executable runs, but only a `.app` bundle gets a dock icon and a proper app name |
| `treesight` (Linux) | links WebKitGTK dynamically, so build it on the distro you will run it on; there is no realistic static option |

The static build also swaps the highlighting regex engine from oniguruma (C) to
fancy-regex (Rust) with `--no-default-features --features pure`, which is what
removes the last C dependency. Output is byte-identical either way; highlighting
is roughly twice as slow and the binary about 1.4 MB larger. That same flag also
makes cross-compiling `treeserve` painless anywhere you have no C toolchain for
the target, though the Windows cross build below does not need it. `treesight`
is otherwise best built natively per platform, since it links that platform's
webview.

### Cross-compiling for Windows from Linux

`./build.sh windows` produces both `.exe` files — binaries only, no installer;
that part of Tauri's bundler is Windows-only. Two tools, neither of which needs
root or distro packages:

```sh
rustup target add x86_64-pc-windows-gnu
cargo install cargo-zigbuild --locked   # or: cargo binstall cargo-zigbuild
# plus zig 0.15+ on your PATH, from https://ziglang.org/download/
```

zig is doing all the real work: it carries a mingw-w64 sysroot, a C compiler
(so oniguruma still builds, and the result keeps the faster default engine), a
linker and a resource compiler. `cargo-zigbuild` points cargo's linker and
`cc-rs` at it. `build.sh` then writes two shims into `target/zig-shims/`,
because a couple of build scripts still reach for binutils by name: `rustc`
wants a `dlltool` for the raw-dylib imports in `windows-sys`, and `tauri-winres`
will only drive a GNU `windres` for the icon, version info and DPI-awareness
manifest — `zig dlltool` and `zig rc` do both jobs behind those names.

The gnu target links WebView2's loader as a DLL, where the MSVC build gets a
static library, so `dist/windows/treesight.exe` travels with the
`WebView2Loader.dll` next to it. `treeserve.exe` stays a single file. To get a
one-file `treesight.exe` — or an installer — build on Windows with `build.bat`.

**Copy the result to a Windows drive before running it.** Started in place, from
`\\wsl.localhost\…\dist\windows\`, Windows sees an unsigned executable arriving
from a network location: it puts up *“can’t verify who created this file”* and
waits, Defender scans the whole image over WSL's 9P transport, and the image
pages in the same way. Measured here, that is 100 seconds the first time and
2.3 s once the scan is cached; the same binary under `C:\` answers in 0.19 s, and
`treesight.exe` opens its window in 0.13 s. Which folder you *serve* costs
nothing — serving the WSL tree from a binary on `C:` is as fast as anything else,
because the server reads it through the same 9P mount either way.

```sh
mkdir -p /mnt/c/WinApps && cp dist/windows/* /mnt/c/WinApps/
```

For a macOS universal binary, build both architectures and join them:

```sh
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
lipo -create -output treeserve \
    target/aarch64-apple-darwin/release/treeserve \
    target/x86_64-apple-darwin/release/treeserve
```

## Usage

```
treeserve [OPTIONS] [ROOT]

  -b, --bind ADDR        address to bind (default: 127.0.0.1)
  -p, --port PORT        port to listen on (default: 8080)
  -t, --theme MODE       default theme: auto | light | dark (default: auto)
      --no-line-numbers  line numbers off by default
      --no-sidebar       side pane off by default
      --hidden           show dotfiles
      --title NAME       site title (default: root directory name)
      --threads N        worker threads (default: 8)
      --syntax-theme NAME        highlighting theme for both modes
      --syntax-theme-light NAME  highlighting theme for light mode (default: InspiredGitHub)
      --syntax-theme-dark NAME   highlighting theme for dark mode (default: OneHalfDark)
      --list-syntax-themes       list embedded highlighting themes and exit
```

All ~30 highlighting themes (Dracula, Nord, Solarized, Catppuccin, gruvbox, …)
are embedded in the binary and selectable at run time; names are matched
case- and punctuation-insensitively (`one-half-dark` == `OneHalfDark`).

Example: `treeserve -p 9000 ~/projects/notes`

## Features

- **Server-side rendering, zero JS.** Every page is plain HTML + CSS.
  Toggles (theme, line numbers, side pane) are links that set a cookie via
  `/.ts/set` and redirect back.
- **File tree side pane.** Rendered on the server; directories on the current
  path are expanded, everything else is a link, so no JS is needed for
  expansion. “Pane” in the header switches the whole pane, not the tree within
  it: the tree is what the pane is for, so turning it off gives the listing the
  full window rather than leaving an empty column.
- **Drawn icons, not typed ones.** Listing rows and header controls use inline
  SVG paths that inherit the surrounding colour, so a directory's icon is
  link-coloured and a file's is muted, and the theme flag shows the state it is
  in: a sun for light, a moon for dark, half of each for following the system.
  The characters you would reach for instead — folder, picture, film, note — are
  all in the emoji planes, and a font stack without them (any DejaVu-only Linux,
  for one) draws a missing-glyph box where the icon should be.
- **Refresh.** A page is a snapshot of a directory taken when it was asked for,
  so the header carries a **Refresh** flag that asks for a new one. One control
  for both halves of the window: the tree in the pane and the file in the middle
  come out of the same request. Pages are sent `Cache-Control: no-store`, and raw
  files carry an `ETag` built from their modification time and length — so a
  reload genuinely re-reads the disk, and a picture that changed is fetched again
  instead of being painted from the browser's copy, while one that did not costs a
  `304`. In the desktop shell the flag is an actual reload (`F5`, `Ctrl+R`), which
  keeps your place on the page and leaves the history alone.
- **Glob filter.** Each directory listing has a filter form (`*.rs`,
  `[a-c]*.md`, …) with an optional recursive mode. Patterns containing `/`
  match relative paths. An unfiltered listing that contains `README.md` (or
  `README.markdown` / `README.mdown` / `README.mkd`) renders that file under
  the table, GitHub-style; glob results and `curl` listings stay names only.
- **Syntax highlighting** via syntect (Sublime Text grammars, ~190 languages
  through two-face), with a line-number gutter that can be toggled per user
  or disabled by default with `--no-line-numbers`.
- **Markdown** rendered server-side by comrak (GFM: tables, task lists,
  strikethrough, autolinks, tag filtering, plus GitHub's footnotes, alerts
  and heading anchors). Fenced code blocks go through the same syntect
  pipeline. A “Source” link shows the highlighted raw file.
- **Mermaid.** Fenced ` ```mermaid ` blocks (and standalone `.mmd` /
  `.mermaid` files) are laid out on the server by mermaid-rs-renderer and
  inlined as SVG — two copies, light and dark, so theme switching stays
  pure CSS, the same way syntax colours do. Output is native-Rust SVG, not
  pixel-identical to mermaid-cli. Diagrams that do not parse are shown as
  source with the renderer message as a tooltip, like broken math. A fence
  larger than 64 KiB is left as source so a huge diagram cannot stall a
  worker.
- **Math.** `$x$` and `\(x\)` inline; `$$x$$`, `\[x\]` and ```` ```math ````
  blocks for display math — the union of what GitHub and KaTeX accept. The
  LaTeX-native `\(…\)`/`\[…\]` pairs are rewritten to the dollar forms before
  parsing (CommonMark would otherwise eat the backslashes as escapes); code
  spans and fenced blocks are left alone, and an opener with no partner in the
  same paragraph stays literal text. Formulas are typeset on the server:
  pulldown-latex converts
  the LaTeX to MathML, so formulas render as text in the browser with no
  JavaScript and no webfont download. Formulas that don't parse are shown in
  red with the parser message as a tooltip. Escape a literal dollar sign as
  `\$`. Rendering quality depends on the browser having a math font
  (Cambria Math on Windows, Latin Modern/STIX/DejaVu Math on Linux,
  built into Firefox); without one, the browser's default math layout is used.
- **Dark / light / auto themes.** Highlighting is emitted as CSS classes;
  one stylesheet per theme is generated at startup, so theme switching is
  pure CSS (`prefers-color-scheme` in auto mode, cookie override otherwise).
- **Media:** images, video, audio (with HTTP Range support for seeking) and
  PDF render inline; binaries get a download page.
- **curl-friendly.** Clients that don't ask for `text/html` get raw bytes
  for files and a plain-text listing for directories:
  `curl host:8080/path/file.tar > file.tar` just works.
- **Safety:** paths are sanitized and canonicalized; symlinks pointing
  outside the served root return 403.

## Query parameters

- `?raw=1` – raw bytes (correct Content-Type, Range supported)
- `?dl=1` – force download (Content-Disposition: attachment)
- `?src=1` – highlighted source view for Markdown and Mermaid files
- `?q=GLOB&r=1` – filter a directory listing (recursive with `r=1`)

## Desktop app — treesight

`shell/` wraps the same server in a Tauri window and `app/` packages it, for
people who would rather double-click an icon than run a command:

```sh
cargo run -p treesight -- [FOLDER]     # dev run
./build.sh bundle                      # installers → target/release/bundle/
```

- **Started with a folder** (argument, shell verb, drag onto the exe) it serves
  it right away. **Started without one** — the normal double-click case, where
  the working directory is whatever the shell happened to pick — it opens on the
  **start page**: what there is to open, being Places, whatever an embedder
  added, and Recent, with a button for the folder picker. Nothing is served
  until something is chosen, and no modal stands in front of a window nobody has
  seen yet.
- **Nothing open is a state**, not a failure: `Config::root()` is an `Option`, the
  start page is what `handle` answers with while it is `None`, and any other path
  redirects there rather than erroring about a root that is not the point. The
  pane's **Files** section — the tree, with the folder picker and a Close on its
  heading — is absent until there is a folder in it. Closing puts the window back
  on the start page; what was open is in Recent, so nothing is lost by it.
- The window opens on `treesight://localhost/`, a URI scheme the app registers
  and answers itself — no socket, no port, nothing for another process on the
  machine to reach. Cookies, redirects, Range requests and relative links work
  exactly as in a browser, because all of that belongs to the router rather
  than to a server. The app does not build one: `tiny_http` is behind the
  `http` feature, which only `treeserve` turns on.
- **The left pane is also the folder chooser**, so choosing one looks the same on
  every platform — which the native dialogs do not, GTK offering *Other
  Locations* where Windows offers *Quick access* and no tree at all. The tree
  has the pane to itself; scroll past it for **Places** (home, desktop,
  documents, downloads, and drive letters or `/`) and **Recent** (the last 8
  roots, kept in `recent.txt` in the app's config dir), which is where
  shortcuts reached for now and then belong. Places never feed Recent: that
  list is fixed, and a shortcut copying itself into the list below it would say
  nothing new. Re-rooting also happens by dropping a folder on the window, and a
  second launch re-roots this window rather than starting a second copy.
- **One line of chrome, not two.** No menu bar — those were rarely-used menus
  holding a permanent row — and no browser-style location bar either. Back
  shares the header with the path and the flags, and the shortcuts are
  `Alt+←` / `Alt+→` (back, forward), `Ctrl+R` or `F5` (reload), `Ctrl+Home` (top of
  tree) and `Ctrl+O` (folder picker). On macOS a menu is also where `Cmd+Q` and
  the clipboard shortcuts come from, so that platform needs its own menu back
  before it is worth shipping there.
- **Nothing gets a row it does not earn.** Every control — Back, Source, Raw,
  Download, the flags — spells itself out while there is room and falls back to
  an icon when there is not, and the pane narrows and then goes as the window
  does. **Open Folder…** therefore lives in the status line rather than the pane,
  that being the one line which is always there.
- **One screen, three regions.** The shell lays itself out as an app rather than
  as a long document: the header and the status line stay where they are, and
  the side pane and the listing scroll independently, so following a deep tree
  never scrolls the file you are reading off the top. A page in a browser keeps
  scrolling like a page, which is what a browser's own chrome expects.
- **Downloads** don't go through the webview's download stack, which is invisible
  on some platforms and missing on others. A `?dl=1` link is intercepted, the URL
  is resolved back to its file with the server's own traversal checks, and a
  native Save dialog copies it straight off disk — no second HTTP round trip.
  Downloads the webview starts by itself (a PDF WKWebView won't render, say)
  still get a destination in the OS download folder and a confirmation.
- Links pointing outside the served tree open in the system browser.

### Why none of that reaches the web

Every control above either re-roots the server or names a path outside it, so a
page served to anything but this window must not offer them. Two things keep
that true:

- `Config::app_ui` decides whether they are rendered at all. It defaults to
  `false`, `treesight` is the only thing that sets it, and there is no CLI flag
  to turn it on — a flag plus `--bind 0.0.0.0` would be a remote filesystem
  browser in someone else's filesystem.
- The controls do nothing on their own. They are ordinary links to `/.ts/open`,
  `/.ts/root`, `/.ts/place` and `/.ts/back`, and **none of those is a server
  route**: `treeserve` never re-roots itself over HTTP and answers 404 for all
  four. The desktop shell recognises them in its navigation handler and cancels
  the navigation, the same way it already intercepts `?dl=1` downloads. The
  capability lives in the one client allowed to have it.

A page served by the CLI therefore carries none of it — no back button, no
Places, no Recent, no picker, and still no JavaScript. What it does share is the
header flags, which now carry a symbol alongside their words for narrow windows.
The one-screen layout it does not: the shell turns that on with a class on
`<body>`, rather than imposing it on a page in a browser. The shell also injects
a small keydown handler for the shortcuts, since it has no menu to carry
accelerators.

### Windows Explorer integration

`app/windows/hooks.nsh` is an NSIS installer hook that registers a **“Browse
with treesight”** verb for folders, folder backgrounds and drives, pointing at
`"$INSTDIR\treesight.exe" "%V"`. It is written under `SHCTX`, so a per-user
install registers in `HKCU` and a per-machine install in `HKLM`, and the
uninstaller removes it. On Windows 11 the entry appears under **Show more
options** — surfacing it in the top-level menu would need an `IExplorerCommand`
shell extension, which this doesn't ship.

### Offline installers

The WebView2 runtime ships *inside* the Windows installer
(`webviewInstallMode: offlineInstaller`), so installing needs no network at all.
That costs about 127 MB of installer size; it is already present on Windows 11
and current Windows 10, so switch to `downloadBootstrapper` in
`app/tauri.conf.json` if you would rather have a small installer and let it
fetch the runtime on first install.

### Do you need the Tauri CLI?

Not for development: `cargo run -p treesight` builds and runs the app, and this
project has no JavaScript frontend, so nothing here needs Node or pnpm. The CLI
is only for packaging (`tauri build` → NSIS/MSI, deb/AppImage, dmg) and helpers
like `tauri icon`. Install it as pure Rust, no Node involved:

```sh
cargo install tauri-cli --version "^2" --locked   # provides `cargo tauri`
cargo binstall tauri-cli                          # or: prebuilt, much faster
```

The server CLI is unaffected by all of this: its HTTP responses are
byte-identical, which `scripts/snap.sh` is there to prove.

## Direct dependencies

`tiny_http` (sync HTTP server, `http` feature — the CLI only), `syntect` +
`two-face` (highlighting),
`comrak` (Markdown), `mermaid-rs-renderer` (Mermaid → SVG), `pulldown-latex`
(LaTeX → MathML). Everything else —
argument parsing, URL handling, templating, glob matching — is hand-rolled
std-only Rust. The `treesight` crate adds `tauri` plus its dialog, opener and
single-instance plugins.
