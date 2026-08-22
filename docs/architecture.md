# Architecture

What this repository is, how a request becomes a page, and where a
downstream app (the SSH product, a separate private repo) is allowed to
plug in. The README is the user-facing manual; this file is for people
changing the code.

**This repo stays local-only.** It never depends on russh, tokio, or any
SSH concept. Remote filesystems arrive as another [`Vfs`](../src/vfs.rs)
implementation supplied by the embedder.

---

## Two crates, two binaries

| crate | path | binary | role |
|---|---|---|---|
| `treeserve` | repo root | `treeserve` | HTTP server + CLI. Sync worker threads, `tiny_http`. |
| `treesight` | `app/` | `treesight` | Tauri window around the same server. `publish = false`. |

`cargo build` at the root builds only `treeserve`, so webview libraries are
needed only when you ask for the app (`cargo run -p treesight`, `./build.sh`).

The CLI (`src/main.rs`) is a thin argument parser over `treeserve::spawn`.
The app (`app/src/main.rs`) is seven lines that call `treesight::run()`.
`run()` is `run_with(generate_context!(), ShellExt::default())`; a second
app supplies its own Tauri context and a [`ShellExt`](#shellext).

One router, two faces. `handle(&State, &Req) -> Reply` decides every answer
and touches nothing that carries it.

```
                    ┌─────────────────────────────────────────┐
  treeserve CLI     │  handle(&State, &Req) -> Reply          │
  http::spawn       │         ▲                               │
  feature = "http"  │         │ Arc<dyn Vfs>  (LocalFs here)  │
                    └─────────┼───────────────────────────────┘
                              │
  treesight window ─ scheme ──┘   treesight://localhost
       │                          http://treesight.localhost on Win/Android
       │  on_navigation intercepts /.ts/open, /.ts/root, …
       │  ShellExt.actions run first (downstream claims)
       └─ native dialogs, Recent file, Places, Save As
```

The app links the router without the server: `default-features = false` drops
`tiny_http` from its graph entirely. Nothing is listening on the app's side, so
there is no address for another process on the machine to find — which is why
there is no token, no cookie handshake and no origin to authenticate.

---

## Request path

Two callers reach the same router.

`http::spawn` binds `cfg.bind:cfg.port` (port `0` lets the OS pick) and starts
`cfg.threads` worker threads. Each loop is `server.recv()`, then `respond`,
which is a four-line adapter: build a `Req`, call `handle`, write the `Reply`
down the socket. A panic in one request is caught; the worker stays up.

The shell registers `treesight` as a URI scheme and answers each webview
request by calling `handle` directly, on a spawned thread — highlighting a
large file would otherwise stop the window painting. A `Body::Stream` is read
into bytes there, because a custom protocol answers with a body rather than a
handle to pull on; the case that would hurt never arrives whole, since the
webview asks for a film a Range at a time.

`handle` (`src/lib.rs`) is GET-only and does, in order:

1. Snapshot `cfg.root()` once for the whole request, so a re-root mid-render
   cannot paint a listing from one tree and a pane from another.
2. Compiled-in assets: `/.ts/app.css`, `/.ts/math.css`,
   `/.ts/syntax-{light,dark}.css`. Preference cookies: `/.ts/set`.
3. `/.ts/wait` — parking page while the shell probes a folder (`app_ui` only).
4. `resolve_in_root` — percent-decode, reject `.` / `..` / NUL, then
   `Vfs::resolve` (symlink canonicalize + confinement). Missing → 404,
   outside the root → 403.
5. Directories 301 to a trailing slash, then `page::listing_page`.
   Files: `?raw=` / `?dl=` / non-HTML / subresource (`Sec-Fetch-Dest`) go
   through `serve_raw` (Range, ETag). Otherwise `view::file_page`.

HTML pages are `Cache-Control: no-store`. Raw bodies get an ETag from
`(mtime, len)` in `Meta`. Theme, line numbers, sidebar and the pane's opened
directories are cookies (`ts_theme`, `ts_ln`, `ts_sidebar`, `ts_open`), not
query strings, so a shared URL stays shareable.

**The shell keeps its own cookie jar** (`Jar`, `app/src/lib.rs`), because a
custom scheme has nowhere to keep cookies. `treesight://localhost` has an
opaque origin — it serializes to "null" — and a scheme request never reaches
the network process that would do the storing, so `Set-Cookie` was dropped and
no `Cookie` ever came back: every toggle returned to an unchanged page. The jar
holds what a reply asked to store, folds it into the `Cookie` on every request
out, and persists to `prefs.txt` beside `recent.txt` so a chosen theme outlives
the process.

`/.ts/set` is answered in `shell_action`, not by a page load — the other half of
the same problem. Its reply is a 303, and a 303 is the one thing a custom scheme
cannot carry out: the webview takes the empty body and stays where it is, so a
stored preference first appeared on whatever page came next. The router still
decides what `/.ts/set` stores; the shell hands the reply to the jar and reloads
the page, which is also what keeps the reader's place in a long listing. Because
nothing writes to the webview's own store any more, the jar wins over a `Cookie`
it still sends — on Windows and Android, where the scheme arrives as
`http://treesight.localhost` and that store is real, it holds only what the
toggles wrote before this jar existed.

`localStorage` would not do instead: an opaque origin has none to reach, and
reading it needs script on the page, which the next paragraph rules out.

There is **no JavaScript on served pages**. Toggles are links to `/.ts/set`.
The only script in the product is the shell's `initialization_script`
(keyboard shortcuts), injected by Tauri into the webview, not into HTML.

The one piece of page state that is neither a cookie nor a link is the
narrow-window drawer (≤50rem, where the pane leaves the layout): a checkbox
that only CSS reads, `#ts-drawer:checked ~ .shell nav.tree`. The sibling
combinator is a constraint on the markup — the input must stay a *preceding
sibling* of `.shell`, which is why `head_and_header` emits it as the first
child of `<body>` while `layout` emits the closing scrim inside `.shell`.
The state is per page, so the drawer closes on every navigation; that is the
intended behaviour, not a limitation being tolerated.

---

## Modules (`treeserve`)

| file | job |
|---|---|
| `lib.rs` | `Config`, `Root`, `State`, `PaneSection` / `PaneEntry`, `Req` / `Reply` / `Body`, `handle`, `state_for`, Range, ETag, `root_id_is_local`, `leaf_of` |
| `http.rs` | `feature = "http"` only: `Serving`, `spawn`, the worker loop, and the tiny_http ⇄ `Req`/`Reply` adapters. The one file that knows a socket exists |
| `vfs.rs` | `Vfs`, `VfsPath`, `LocalFs`, `Meta`, `Entry`, `ResolveError` |
| `page.rs` | Listing, tree pane, Places/Recent/sections, layout, wait/error pages, SVG icons |
| `view.rs` | File page: media, PDF, markdown, mermaid files, highlighted source |
| `md.rs` | comrak + syntect fences; mermaid → dual light/dark SVG; LaTeX → MathML |
| `hl.rs` | syntect / two-face; class-prefixed CSS generated at startup |
| `util.rs` | HTML/percent/glob, `display_path` (strips `\\?\`), media extension lists, `MAX_HIGHLIGHT_BYTES` (2 MiB) |
| `app.css` / `math.css` | compiled in with `include_str!` |

`main.rs` is CLI-only and does not sit on the request path.

Caps that keep a worker from stalling on a huge file: 2 MiB for highlight /
README / markdown body; 64 KiB for a mermaid fence (`md.rs`). Anything
bigger is offered as Raw / Download, or left as source.

---

## VFS and RootId

Every byte the renderer needs comes from `Vfs`. `LocalFs` is `std::fs`
behind that trait. Paths inside a root are `VfsPath` — slash-separated
segments, never a host `PathBuf` — so a remote backend does not have to
pretend to be Windows or Unix.

**Confinement is `resolve` only.** It canonicalizes (follows symlinks) and
refuses a target outside the served root. `read` / `read_dir` / `metadata`
are then asked about that canonical path, or about children made by joining
names `read_dir` returned (README preview, tree walk, search). Those joins
follow symlinks the way `std::fs` always did: a URL cannot *navigate* out,
but a listing may still *summarize* a link. A stricter backend may confine
every method; the renderer only depends on `resolve` for 403.

**RootId** is a scheme-aware string naming a served root:

- Local: the bare **display-form** host path — the same string `recent.txt`
  and `/.ts/root?path=` have always carried. `display_path` strips Windows
  `\\?\` prefixes so the id matches what a person sees.
- Remote (downstream): `ssh:<bookmark>:/absolute/path`. A prefix of two or
  more characters that looks like a URI scheme marks a remote id;
  a single letter before `:` is a Windows drive. `root_id_is_local` is the
  only grammar — do not re-derive it.

`Root { id, vfs }` is swapped together (`set_root` / `set_root_vfs`). A
page never gets a title from one root and a tree from another.

`root_id_at(path)` is what the tree's “serve this folder as the root” link
puts in `?path=`. For `LocalFs` that is `display_path` of the host path.

---

## treesight

The window is a webview opened on `treesight://localhost/`, served by a URI
scheme the shell registers and answers out of `handle`. Windows and Android
hand a registered scheme to the webview as `http://treesight.localhost`
instead; `scheme_base()` is the one place that knows the difference.

Cookies, redirects, Range and relative links all still work, because every one
of them belongs to the router rather than to the socket that used to carry it.

`Serving` holds the `Arc<State>` the router serves out of, and `Serving::entry`
is now simply the served root — there is nothing to collect on the way in.

### What the server must not do

Anything that re-roots or names a path outside the served tree is the
shell's capability:

- `Config::app_ui` (default false) decides whether Places, Recent, Back,
  Refresh, Open Folder, Print, wait page, extra sections are **rendered**. Only
  treesight sets it. There is no CLI flag — a flag plus `--bind 0.0.0.0`
  would be a remote-filesystem proxy.
- The controls are ordinary links (`/.ts/open`, `/.ts/root`, `/.ts/place`,
  `/.ts/back`, `/.ts/reload`, `/.ts/print`, `/.ts/forget`, `/.ts/close`).
  **None of those is
  a server route**; the CLI 404s them. The shell's `on_navigation` cancels the
  navigation and does the work. `/.ts/set` and `/.ts/tree` are server routes the
  shell answers itself as well — see the cookie jar above.

`?dl=1` is intercepted the same way: `resolve_in_root`, then a native Save
dialog (non-blocking, main thread), and the copy itself — `Vfs::open` →
`File::create` → `io::copy` — on a `std::thread`, so a backend reading over a
network cannot freeze the window for the length of the transfer. A failure
comes back through `run_on_main_thread`. Permission bits from `Meta.mode` are
restored on Unix.

### Nothing open

`Config::root()` is `Option<Arc<Root>>`. `Config::rootless()` is where the shell
starts; `Config::new(dir)` is the CLI's, and re-rooting fills the slot in.

While it is `None`, `handle` answers `/` with `page::start_page` and redirects
everything else there — a URL from before the folder was closed is history, not
an error. The page names itself once — an `h1` in the content, not in the header bar as
well — and takes its words from the embedder: `Config::app_name` and
`app_version` come from Tauri's product name and version (so a shell embedding
this crate stops calling itself treesight in its own window title and status
line), and `Config::intro` from `ShellExt::intro`.

`start_page` and `/.ts/wait` are drawn by `rootless_page`, a skeleton
of its own rather than `layout` with the parts switched off: crumbs, the pane
flag, line numbers and the path in the footer are all about a root, and a header
full of controls for a folder nobody chose is chrome pretending. What stays is
the name (`Config::app_name`, the embedder's, since this crate's own is a
library), the theme control, and the way in.

The start page's three lists — Places, embedder sections, Recent — are built from
the same `Row`/`root_list` renderer the pane uses, so the rows, their greying and
their buttons are decided in one place. It shows **no pane**: a sidebar of
shortcuts beside a page of the same shortcuts is one list twice, and the pane
earns its place once there is a tree in it. `Config::picker` says whether this
platform can be asked for a folder at all; where it cannot, the page names the
Places instead of drawing a dead button.

### Re-root

`open_root` canonicalizes off the UI thread (a dead mapped drive can sit
for ~20 s). The window shows `/.ts/wait` meanwhile. Success: `set_root`,
`remember_root_id`, navigate to `entry`. Failure: `RootStatus` on the pane
(missing vs unreachable — Windows `ERROR_BAD_NETPATH` is not `NotFound`).

`check_roots` probes Places and Recent **local** ids only, one thread per
path, and prunes dead Recents from `recent.txt`. Remote ids are skipped;
whoever supplied them owns their status via `set_root_status`.

Places come from the platform (home, desktop, documents, downloads, drive
letters or `/`). Recent is `recent.txt` in the app config dir, newest
first, max 8, RootId strings. Opening a Place does not write Recent.

The tree sits in a **Files** section, headed like Places and Recent, and that
heading carries both ends of having a folder open: the picker, and `/.ts/close`
(`Config::close_root`) which puts the window back on the start page. What was
open is in Recent, so there is nothing to confirm. The picker also appears in the
status line, but only below 50rem where the pane has left the layout, so no width
offers two — and only where `Config::picker` says the platform has one at all. A section's
`heading_action` is `(href, icon, title)`: the mark is the embedder's, because a
plus promises adding one thing and a list of servers is *managed*.

Every Recent row carries a Forget button — `/.ts/forget?path=`, answered by
`forget_root_id`, which drops the id from the list and the file and reloads the
page. Recent alone gets one: it is the only pane list that is a record of what
the reader did rather than a fixture, so the only one that can hold something
they want gone — a folder that moved, or a root some since-fixed bug wrote down
wrong. It forgets the row and never touches the folder.

### The tree pane

A directory row is three controls, not one: the arrow opens it here, the name
walks into it, and (in the shell) the button re-roots to it. The arrow used to be
a character *inside* the name's link, so clicking it walked in — there was no
second control to click.

What is open is the **union of two sources**, kept apart on purpose:

- the **implicit** chain from the root down to the current directory, which is
  never stored: it follows from where you are, storing it would fill `ts_open`
  with everywhere you had been, and a collapse of it could hide the row you are
  standing in. Those arrows are inert markers with no link.
- the **explicit** set in `ts_open`, which is every arrow you clicked.

`/.ts/tree?open=<rel>` and `?shut=<rel>` each change one entry and 303 back to
`back=`. The direction is in the link rather than being a toggle, because a
toggle is wrong the second time it is followed — which is what a double click and
a reload both are. `open_path` re-checks every entry on the way in and out: the
list is walked to *draw* the pane, before `resolve_in_root` guards anything.

The set is capped at `OPEN_MAX` (24) entries and `OPEN_COOKIE_MAX` (3000) bytes,
oldest dropped first. Each entry costs one `read_dir` per render — a network
round trip on a remote backend — and a cookie a browser silently drops is a pane
that forgets everything. `TREE_MAX_PER_DIR` (150) still caps each directory's
rows.

### Navigation allowlist

The window may only stay on:

1. The shell's own pages, matched by `origin_allowed(shell_origins(), …)`.
   Two forms, because a URL on a non-special scheme has an **opaque** origin:
   `treesight://localhost` serializes to `"null"`, so that half matches by
   scheme, and `http://treesight.localhost` matches as a whole origin. Never by
   prefix — `http://treesight.localhost.evil.com` starts with the same text.
2. `ShellExt.allowed_origins` — the same two forms, for a downstream scheme
   (`telesight:`, `http://telesight.localhost`).

Anything else opens in the system browser.

### What may run on the navigation callback

`on_navigation` runs on the thread the window answers input on, and on Android
that is the **Java UI thread** — which is also the thread that services every
message wry posts to itself. A call that waits for the webview to answer is
therefore a call waiting for the thread it is running on:

| Call | Waits on | Timeout |
|---|---|---|
| `WebviewWindow::url()`, `cookies()`, `webview_version()` | wry's MainPipe | 10 s |
| anything through `run_mobile_plugin` — **every** `app.path().*_dir()`, `opener().open_url()` | the same pipe | none at all |

The symptom is not a crash: the window stops repainting, `adb logcat` says
`Input dispatching timed out`, and there is no tombstone and no panic. Nothing
failed, something waited.

Two rules, both cheap:

- **Spawn.** Anything reached from the callback that is not pure computation
  goes on a thread — `set_pref`, `save_as`, `open_root` and the external-link
  opener all do. From a worker thread every one of those calls completes,
  because the looper is free to answer it, and one thread per click is nothing.
- **Never decide anything from a tao window getter on Android.** `is_visible()`
  warns and returns false, `title()` returns `""`, `set_title` and `set_visible`
  are no-ops. Gate the check with `cfg!(desktop)` instead of believing it.

Safe on the callback: `navigate` and `eval` (posts with no reply channel), every
tauri-plugin-dialog dialog (its mobile backend wraps each one in a thread of its
own), and reads of state or files.

---

## ShellExt

Unstable embedder API. `run()` is the public app; a downstream binary
calls `run_with(ctx, ext)`.

| field | purpose |
|---|---|
| `actions` | Tried **before** built-in `shell_action`. Return true to claim the URL. Remote `/.ts/root?path=ssh:…` must be claimed here or the built-in will refuse it. |
| `extra_places` | Extra rows **inside** Places. |
| `extra_sections` | Whole headed lists between Places and Recent (`PaneSection`). Evaluated at server start; later updates go through `Config::set_sections`. |
| `intro` | The sentence the start page opens with. A downstream app is a different program; the default sentence is about this one. |
| `init_script` | Replaces `SHORTCUTS` wholesale (needed so a terminal page can keep Alt+arrows). |
| `allowed_origins` | Extra origins the webview may load (plugin scheme pages). |
| `configure` | One shot at the Tauri `Builder` (plugins). Runs after dialog/opener; single-instance stays first. |
| `openers` | `RootOpener`s for roots that are not paths. `claims` picks one, `label` names it on the wait page, `open` runs on a thread of ours and returns `Opened { id, vfs, name }` or `None` — `None` meaning the opener has already said why, or that nothing needed saying. `probe` answers for a row that is only being listed and **must not connect**. |

Public helpers the embedder is meant to call:

- `Serving::state` / `origin` / `entry`, `WINDOW`
- `remember_root_id(app, id)` — Recent, disk, status Ok
- `Config::set_root_vfs`, `set_sections`, `set_root_status`
- `replace_page(app, url)` — **use this rather than `WebviewWindow::navigate`**
  for anything the shell puts on screen itself: a re-root, a wait page, putting
  back the page a cancelled action came from. Every page of this window wears
  the same address, so a navigation pushes an entry that is a copy of the page
  you are on, and Back or Android's swipe then appears to do nothing. A link
  somebody followed is the exception and should navigate.

`RootOpener` is the seam for a root this crate cannot open. The choreography
around one is here and happens once — wait page off the callback, opener on a
thread, `set_root_vfs`, `set_root_name`, Recent, replace the page, show the
window; and on `None`, the page that was on screen comes back. `check_roots`
routes a non-path id to its opener's `probe` rather than skipping it, which is
what lets a revoked grant or a server that is not answering be grey before it is
clicked.

`PaneEntry` links are always `{action}?path={percent_encode(id)}`. Aside
links on a row are raw hrefs (e.g. a terminal URL). A heading action is
`(href, title)` drawn as a plus.

---

## Security (local product)

- The app opens no socket at all: it registers a scheme and calls the router.
  There is no port for another local process to reach, and so nothing to
  authenticate to. Authentication, TLS and anything in front of them belong to
  `http.rs`, which the app does not compile.
- Default bind `127.0.0.1` for the CLI, which can bind elsewhere; `app_ui`
  cannot be turned on from the CLI.
- Symlink escape → 403 for **navigation**.
- No secrets in `recent.txt`. No `?password=` links.
- Offline: no CDN, no webfonts, no telemetry. Syntax themes, mermaid,
  math, CSS are compiled in.
- Downloads resolve through the same `resolve_in_root` as pages.

---

## Tests and snapshots

`cargo test` (treeserve) and `cargo test -p treesight`. After treeserve
changes, two feature builds must still compile: `--no-default-features
--features pure` (fancy-regex instead of oniguruma; used by the static musl
build) and `--no-default-features --features onig` (no `http`, which is what
the shell builds — it catches anything that has quietly grown a dependency on
the server).

`scripts/snap.sh` is the HTML/HTTP A/B harness: two servers over one
fixture (CLI-shaped and `app_ui`). Capture before and after a
change, `diff -r` the directories. Byte-identical refactors should diff
empty; markup changes should contain only the intended delta.
`examples/snapshot_server.rs` is the fixture server.

---

## What this repo is not

SSH browsing, a terminal, bookmarks, russh, and mobile SSH UI live in a
**private companion repo** (working name **telesight**). That repo vendors
this one and fills `Vfs` + `ShellExt`. Do not add russh here. If a seam
is missing, add a *neutral* capability in this repo (another `Vfs` method,
another `ShellExt` hook) rather than an SSH type.

The CLI will not grow `treeserve user@host:`.
