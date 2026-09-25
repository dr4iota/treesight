# Architecture

What this repository is, how a request becomes a page, and where a
downstream app (the SSH product, a separate private repo) is allowed to
plug in. The README is the user-facing manual; this file is for people
changing the code.

**This repo stays local-only.** It never depends on russh, tokio, or any
SSH concept. Remote filesystems arrive as another [`Vfs`](../src/vfs.rs)
implementation supplied by the embedder.

---

## Four crates, two binaries

| crate | path | binary | role |
|---|---|---|---|
| `treeserve` | repo root | `treeserve` | HTTP server + CLI. Sync worker threads, `tiny_http`. |
| `treesight-shell` | `shell/` | — | The window, the custom scheme, the panes — everything the app is made of, as a library. |
| `treesight` | `app/` | `treesight` | This repo's app: a tauri.conf.json, icons, and the context they compile into. `publish = false`. |
| `treestamp` | `stamp/` | — | What a build says it is. No dependencies: it is a build-dependency *and* a dependency of every consumer — `treeserve`, `treesight-shell` and `treesight` all stamp themselves with it. |

`cargo build` at the root builds only `treeserve`, so webview libraries are
needed only when you ask for the app (`cargo run -p treesight`, `./build.sh`).

The CLI (`src/main.rs`) is a thin argument parser over `treeserve::spawn`.
The app (`app/src/main.rs`) is twenty lines: read `--version`, then hand
`generate_context!()` and a [`ShellExt`](#shellext) to `treesight_shell::run_with`.
A downstream app does exactly the same with its own context and its own
extensions — this repo's app gets no shortcut the others do not.

**Why the shell is a separate crate from the app.** `tauri_build::build()`, which
every Tauri *app* runs, rewrites the Gradle module list of whatever
`TAURI_ANDROID_PROJECT_PATH` names — from the plugin set of the crate calling it.
When a downstream Android app links a shell that also calls it, both build scripts
write that file in the same second and the loser's plugins vanish from the build,
surfacing a build later as a `ClassNotFoundException` on launch. So the library
half calls it nowhere and sets the `desktop`/`mobile` cfg aliases by hand instead;
only `app/` builds a Tauri context, and downstream apps build their own.

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

**The shell keeps its own cookie jar** (`Jar`, `shell/src/lib.rs`), because a
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

A reload is how every change of state in this window arrives, and a reload puts
every scroller in the page back at the top — the pane and the listing are each an
`overflow: auto` box rather than the document's scroller, and no engine restores
one of those (measured: an identical document, reloaded, comes back at zero). So
opening one directory in a long tree threw the reader to the top of it. The
shell's `SCROLLERS` script keeps both offsets in a fragment of its own
(`#ts<pane>,<listing>`), written with `location.replace`, which costs no history
entry and stays a same-document change. The fragment is always written, never
removed: a replace that takes the fragment *off* is a navigation to the bare
address, which is the address you are on, which is a reload — scrolling back to
the top reloaded the page under the reader until that was understood. Storage was
the other way and there is none here, for the reason the cookie jar exists.

There is **no JavaScript on served pages**. Toggles are links to `/.ts/set`.
The only script in the product is the shell's `initialization_script` —
`VIEWPORT` plus the keyboard shortcuts, or plus whatever an embedder's
`init_script` replaces the shortcuts with (telesight's carries its prompt
overlay too) — injected by Tauri into the webview, not into HTML. Content is
held to the same rule by the `Content-Security-Policy` every page carries
(`html_reply`): raw HTML in a rendered document cannot script, whichever
backend served it.

Script is not the whole of it, though, and the rest is why `md::RawHtml`
exists. `style-src 'unsafe-inline'` is what lets a page carry its theme, and
it is also enough for a document to paint a box over the page and dress it as
the program's own — no script, so no CSP to stop it. A document's own raw HTML
is therefore *drawn* only where it came off this machine
(`Vfs::on_this_device`: `LocalFs`, `EmbeddedFs`, and an embedder's own
device-local backend); from anywhere else the markup is escaped and shown as
the text it is. The line is the transport, not the bytes — a cloned repository
is somebody else's text on your own disk — because that is the line a browser
draws too.

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

**A refusal has three words, and a page must pick one.** `ResolveError` is
`Missing`, `Outside`, `Denied`, `Unreachable` — the last two added because with
only the first pair a backend had to spell *every* failure as "nothing there".
A share whose host had gone gave a 404, and so did a folder whose permissions
merely exclude you: the one answer that sends a reader to look somewhere else,
given for the two situations that are not about somewhere else at all. The words
are deliberately the ones a shortcut row already draws (`RootStatus`), so the
seam, the page and the pane say one thing.

`ResolveError::of(&io::Error)` is the single classifier — `NotFound` is missing,
`PermissionDenied` is denied, anything else is a backend that did not answer —
and `as_reply()` turns that into the status and the two words on the error page.
Both are used past `resolve` as well, because `resolve` is rarely where a remote
root fails: SFTP `realpath` on a modern OpenSSH is lexical and says yes to a path
that is not there, so `metadata` is the first call that can tell. A backend
therefore owes an honest `io::ErrorKind` from *every* method, not only a variant
from `resolve`.

**And a refusal is not an empty directory.** `read_dir_sorted` hands the error
back rather than ending in `unwrap_or_default`, and each surface says what is
worth saying there: the listing replaces its table with a sentence, the pane
marks the node *gone* / *not readable* / *no answer* in the slot "… N more"
uses, `listing_text` returns the error so a script gets a status code, and the
search walk skips the directory and carries on. The HTML listing stays 200 —
knowing the code first would mean listing the directory twice, which over a
remote root is a round trip bought for a number no window here displays.

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
an error. `HOME_PATH` (`/.ts/home`) answers with that page *whatever* is open,
and closes what is; see **Back** below for why the start page needs a second
address of its own. The page names itself once — an `h1` in the content, not in the header bar as
well — and takes its words from the embedder: `Config::app_name` and
`app_version` come from Tauri's product name and version (so a shell embedding
this crate stops calling itself treesight in its own window title and status
line), and `Config::intro` from `ShellExt::intro`.

`start_page`, and `/.ts/wait` before there is a root, are drawn by
`rootless_page`, a skeleton of its own rather than `layout` with the parts
switched off: crumbs, the pane flag, line numbers and the path in the footer are
all about a root, and a header full of controls for a folder nobody chose is
chrome pretending. What stays is the name (`Config::app_name`, the embedder's,
since this crate's own is a library), the theme control, and the way in.

**The wait page reads nothing.** With a root open it keeps that root's chrome —
crumbs, the machine's tag, Refresh, the theme, the status line — because that is
still what is being served, and because every part of it is an id or a
preference rather than a read. What it does not keep is the pane, and
`waiting_layout` is a third skeleton for exactly that reason. The pane is a read
per directory: the root, each directory the reader has opened under it, and a
stat for every symlink on the way. Over a remote root those are round trips, and
this is the one page in the app that must not wait on the network to appear — an
embedder that asks for a secret asks *through a document*, and until this page
commits, the ask lands on the page it is replacing, which cannot tell itself from
a page the reader walked back to. A pane can outlast the budget for finding a
page that will take the question, and then the connection fails for want of the
very page being drawn for it. The pane switch and the drawer button go with the
pane, which is right either way: there is nothing behind them here. Held by a
test — the wait page over a root whose `Vfs` panics on every method.

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
whoever supplied them owns their status via `set_root_status`. It runs once,
when the window is built — so an embedder that greys a row of its own is the
only thing that can un-grey it, and has to say so on every route back, not just
the one that goes through `RootOpener`.

`RootStatus` has three things to say and they send a reader three different
places: `gone` (it answered, nothing there), `denied` (it answered and would not
have us — a permission, a login), `N/A` (it did not answer — a machine or a
link). `denied` and not *refused*: a *connection refused* is a socket with
nothing behind it, which is the third case and the opposite of the one that word
would be naming. The row abbreviates; `cannot_open` spells each out, and matches
on every status by name so a fourth cannot fall through to a sentence meant for
another. The fourth is `Other` (`error`, *could not be opened*), for an
embedder whose backend fails in a way none of the three names.

**What logic reads and what is drawn are kept apart.** `RootStatus` is the
verdict: pruning, `cannot_open`, and an embedder's own bookkeeping all branch on
it, and a fault (`is_fault`) is dimmed. `RootNote` wraps it with what is only
drawn — a `Tone` (`Plain`, `Good`, `Warn`, `Bad`, coloured from `--tip`,
`--warning`, `--err`), a word, and a tooltip (`detail`) — each falling back to the
status's own when `None`, so `set_root_status` draws what it always did. The row
shows the word in the tone's colour; with no word and a tone, a dot, which is how
a healthy row says something good (a connected server) without spending the
width a word would; with neither, nothing. The dot is named for screen readers
by `detail`, or by its tone. Faults always have a word: amber and red dots differ
only by colour, so they are for sparing use.

Pages are static, so a note that changes while one is on screen used to wait for
the next render. Every row carries `data-root`, and `repaint_notes(app, ids)`
evaluates a script that swaps those rows' class and note in place, from the same
renderer the page uses. A page without such rows is untouched, which is what
makes it safe to send to whatever the window is showing.

Places come from the platform (home, desktop, documents, downloads, drive
letters or `/`). Recent is `recent.txt` in the app config dir, newest
first, max 8, RootId strings. Opening a Place does not write Recent.

The tree sits in a **Files** section, headed like Places and Recent, and that
heading carries both ends of having a folder open: the picker, and `/.ts/close`
(`Config::close_root`) which puts the window back on the start page. What was
open is in Recent, so there is nothing to confirm. The picker also appears in the
status line, but only below 50rem where the pane has left the layout, so no width
offers two — and only where `Config::picker` says the platform has one at all. A section's
`heading_acts` are `(href, icon paths, title)` triples: the marks are the
embedder's, because a plus promises adding one thing and a list of servers is
*managed*.

Every Recent row carries a Forget button — `/.ts/forget?path=`, answered by
`forget_root_id`, which drops the id from the list and the file and reloads the
page. Recent alone gets one: it is the only pane list that is a record of what
the reader did rather than a fixture, so the only one that can hold something
they want gone — a folder that moved, or a root some since-fixed bug wrote down
wrong. It forgets the row and never touches the folder.

### Back

Two addresses, and exactly one step between them. Every page of a tree wears
`entry` — the origin and a slash — so an entry pushed for one of them is a copy
of the address you are already on, and Back into it re-renders whatever root is
current now. That is why `replace_page` exists and why everything uses it.

The start page is the exception, and `HOME_PATH` is what makes it one: a second
address, rendering the start page whatever is open and closing what is. So
opening the **first** folder of a run can be a real step — `show_tree` pushes
when the window is showing that page and replaces otherwise, and `show_waiting`
takes the step ahead of it so that a slow dial still costs one entry. Back out of
the tree then lands on a page that says nothing is open and has made that true.

`Serving::at_home` is how the shell knows which it is showing. Not
`WebviewWindow::url()`: that waits on wry's pipe, and `serve_root` runs on the
thread that services it (see *What may run on the navigation callback*). The flag
is written by whatever puts a page up and again by the page-load hook, which is
the only thing that sees a page the shell did not ask for — a link into a
subfolder, and a Back onto the start page.

**Where the step actually comes back.** On the schemes that map to
`http://<scheme>.localhost` — Windows, Android — Back traverses it and lands on
the start page, which is what a phone's system gesture walks. Under a custom
scheme on WebKitGTK it does not: the entry is pushed and Back does not come back
out of it, the same family of trouble that makes the History API unusable on
`tauri://` there. Nothing depends on the step working: Start in the header and the
× on the Files heading go to the same place by the same route (`/.ts/close`), and
they are how you leave a folder on a desktop. Do not "fix" this by pushing more entries,
or by giving the tree a per-folder address — that is the copy-of-one-address
problem above, and it is worse than a Back that does nothing.

**Every root arrives through `show_tree`**, the ones this crate opens for itself
included. Usage is already in memory and has no wait page to step off, which is
how it came to replace instead: over the start page that spent the only entry
the window had, and Back out of Usage left the app rather than landing where
Back out of a folder lands. Judging push-or-replace belongs in one place.

**A replacement needs a page to replace.** `replace_page` evaluates
`location.replace`, which runs in whatever document is current — and when a
navigation of ours has been posted but has not committed, that is still the
document the navigation is about to take away. The replace then races the
navigation instead of replacing it, and the loser is sometimes the folder:
`Files` on Android opens too fast for its own wait page to land, so the wait
page occasionally won and the window sat on "Opening…" with the root open
behind it. And the window was not always a matter of milliseconds: a wait page
drawn while a root is open used to be `layout` like any other page, pane and
all, so over a remote root the renderer had `read_dir`s to make across the
network before the page could even be handed to the webview — while the open it
was waiting for ran on a thread that owed it nothing and may have been reusing a
pooled session. That was the same stranding, on a server, with seconds to happen
in. `waiting_layout` has since taken the pane off that page, which shortens the
window without closing it: a load in flight is still a load in flight, and
`location.replace` still has nowhere to run while it is.
`Serving::loaded` therefore tracks the page *on screen* — false from
the moment we navigate, true again when the webview reports a load started —
and while it is false a page goes up by navigating, which supersedes the load
in flight and costs no entry either.

**Every `eval` that steers the window owes the same question**, which is what
`committed()` is for. `open_failed` did not ask it and was the second victim:
its `history.back()` off a wait page is script like any other, so an open that
failed before the wait page committed walked the *outgoing* document's history
and then vanished with it, leaving the window on "Opening…" with nothing left to
leave it. Fast failures are the ones that hit it — a pooled session refusing a
path in a round trip or two, with the embedder's dialog fire-and-forget so the
failure path runs while the reader is still reading it. Nothing has been stepped
onto in that case, so the answer is to navigate to the page we wanted, exactly as
`replace_page` does.

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

Anything else is handed to the OS through `open_externally` — but only if its
scheme is `http`, `https`, or `mailto`. The bytes reach that callback from
served content (a link in a hostile README), so the browser hand-off is an
allowlist, not a pass-through: `javascript:`, `data:`, `file:`, `intent:`,
`content:` and the rest are dropped, never forwarded, so a README cannot fire an
Android Intent or reach a local file through the app's own opener. The CSP on
`html_reply` stops such a URL from *running*; this stops the app from
*launching* it. `on_navigation` and `on_new_window` share
[`open_externally`](../shell/src/open.rs), and so does an embedder that has no
real `<a>` to click — a terminal OSC 8 link invokes it rather than synthesizing
a navigation.

### What may run on the navigation callback

`on_navigation` runs on the thread the window answers input on, and on Android
that is the **Java UI thread** — which is also the thread that services every
message wry posts to itself. The rule is about that thread, not about the one
callback: `run_on_main_thread` posts to it too, so a closure handed there is
under the same restrictions as the callback that scheduled it — which is why
`serve_opened` and `serve_root` hand Recent's file work to a thread of its own —
and so is every other webview callback (`on_download` resolves its directory
once, on a thread, before any download asks). A call that waits for the webview
to answer is a call waiting for the thread it is running on:

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

### When the last window closes on a phone

tao closes the window when its activity is destroyed, and tauri exits the process
when the last window goes. On Android that is not always the reader leaving. A
cached, frozen process is handed the destroy of an activity that finished
while it slept together with the create of the one a launcher tap just
started. The exit then kills the new activity after it has drawn: the app
flashes and is gone, and `dumpsys activity exit-info` records `EXIT_SELF` in
the foreground.

So `run_with` refuses that exit, waits 300 ms for the destroy to finish
unregistering its activity, and then `reopen` asks tao whether any activity is
left. If one is, the window is rebuilt on it (`build_window`, on the start
page). If none is, the app exits as before. The question has to go to tao's
activity list (`next_available_activity`, reached through `tauri_runtime_wry` so
it is always tauri's own tao). A window build is no test: it binds to the
activity that is being destroyed, and the next launch is blank.

Two reproductions, on a device, after any tauri upgrade:

- **The race.** Background the app, then
  `adb shell am start -f 0x10008000 -n <pkg>/<activity>`. CLEAR_TASK destroys
  the old activity and creates the new one together. Pass: the same pid, and
  the start page drawn.
- **The reader leaving.** Turn on Developer options › *Don't keep activities*
  in Settings. `settings put global always_finish_activities` does not reach
  the activity manager. Then open the app and press Home. Pass: the process
  exits in the background, and the next launch is a normal cold start.

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
| `picker` | Whether *this* shell can ask for a folder where the crate cannot — the grant flow on Android, and the only way it is reachable without a desktop dialog. |
| `usage_pages` | `(path, bytes)` pages of the embedder's own, merged into the Usage root; a path already taken replaces the built-in page. |
| `flags` | Embedder controls on the header's flag row, installed at server start; anything that comes and goes later uses `Config::set_flags`. |
| `allowed_origins` | Extra origins the webview may load (plugin scheme pages). |
| `build` | The downstream binary's own `BuildInfo`, for the footer and its `--version`. `None` means this crate's, which is right only when this crate is what is running. |
| `configure` | One shot at the Tauri `Builder` (plugins). Runs after dialog/opener; single-instance stays first. |
| `openers` | `RootOpener`s for roots that are not paths. `claims` picks one, `label` names it on the wait page, `open` runs on a thread of ours and returns `Opened { id, vfs, name }` or `None` — `None` meaning the opener has already said why, or that nothing needed saying. `probe` answers for a row that is only being listed and **must not connect**. |

Public helpers the embedder is meant to call:

- `Serving::state` / `origin` / `entry`, `WINDOW`
- `theme` / `set_theme` — the jar, for a scheme page that sits beside the tree
- `open_externally` / `may_open_externally` — system browser or mailer; the
  same helper and allowlist `on_navigation` uses, so an embedder with no real
  `<a>` does not copy it (`shell/src/open.rs`)
- `remember_root_id(app, id)` — Recent, disk, status Ok
- `Config::set_root_vfs`, `set_sections`, `set_root_status`, `set_root_note`;
  a `PaneEntry` may carry a `note` of its own, drawn until a status is set for
  its id — sections are built before any `Config` exists to set one on
- `repaint_notes(app, ids)` — the notes on those rows, updated on the page
  already showing, without a reload
- `RootOpener::let_go(app, id)` — called once no row reaches `id` (not pinned,
  not in Recent, not served), including when Recent's overflow pushes it off,
  so an opener can give back what it holds for it: Android folder grants
- `DialogTurn::take()` — the one file dialog the window may have open, desktop
  only; an embedder's own pickers take it too, and ignore a press that finds it
  held. Android guards the same thing in the plugin that launches the picker,
  since a turn held in the process could outlive an activity recreated under it
- `Config::set_flags(Vec<HeaderFlag>)` — controls of the embedder's own on the
  header's flag row, drawn with Refresh and ahead of the page's own. A mark
  (SVG path data, wrapped in this crate's own box), a word, a title and an href
  the shell claims in `actions`. Set as the root changes: a control that acts on
  the root has nothing to act on when the root is a kind it does not know.
- `Vfs::downloadable()` — whether a copy of a file in this tree is worth
  offering, defaulting true. Always the same for a whole tree, which is why the
  backend answers and not the file: `SftpFs` yes, `EmbeddedFs` no (the Usage
  pages are inside the binary already), `LocalFs` yes except on Android, where
  the only local roots are the app's own directories. False draws no control, no
  sentence in a panel, and turns a typed `?dl=1` back into the ordinary view.
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
links on a row are raw hrefs (e.g. a terminal URL). A heading action is one of
`heading_acts`' `(href, icon paths, title)` triples — the mark is the
embedder's, not a fixed plus.

---

## The build stamp

`treestamp::Stamp::emit`, called from `app/build.rs`, hands rustc the version,
the commit, the branch, the dirty-file count and the build time as
`rustc-env`; `treestamp::build_info!("TREESIGHT_")` in `app/src/main.rs` reads
them back into that binary's `BUILD`, which reaches the shell as
`ShellExt::build`. It shows up in `treesight --version` and, as
`Config::app_commit`, in the footer of every page: `treesight v0.1.0
(a1b2c3d4+2)`.

**`treeserve` stamps itself the same way**, from `build.rs` at the repo root
under `TREESERVE_`, and `src/main.rs` puts it in four places: the header of
`--help`, the whole of `--version`, the line printed when the server starts,
and — by setting `Config::app_name`/`app_version`/`app_commit` — the footer of
every page it serves. The one thing the CLI must do that `app/` need not is
`.repo(env!("CARGO_MANIFEST_DIR"))`: `Stamp::new` asks git about the manifest
directory's *parent*, which is this repository for `app/` and the repository
that vendored us for the root crate.

Why it is worth the build script: a binary copied to another machine is
otherwise unidentifiable, and `treeserve 0.1.0` is the same string in every
build ever made. `--version` now answers *which* one, whether the tree it came
from was clean, and when:

```text
treeserve 0.1.0  2026-09-12T18:29:12Z
commit 9cf63e5b  2026-09-12T13:29:17-04:00 on main (clean)
```

Two lines, and the times fall in the same column without being padded to it —
`name 0.1.0` and `commit a1b2c3d4` are both fifteen characters for the names
and versions this repo has. A third line names a vendored checkout where there
is one.

That last part needed a fix in `treestamp` itself. `git()` treats empty output
as no answer, which is right for a hash or a date and wrong for a count: a
clean tree prints nothing from `git diff --name-only HEAD`, so `dirty` came
back empty — the value that means *nobody asked* — and no `--version` in any of
the three binaries could say `(clean)`. The dirty count reads through
`git_said`, which keeps the distinction.

The shell stamps itself too, under `TREESIGHT_SHELL_`, and `run_with` falls back
to it when `ShellExt::build` is `None`. That fallback names the *library*, so a
footer reading `treesight-shell` means a program forgot to say what it is. Parentheses because the footer already spends `·` on the gap
between that label and the path beside it.

Two rules it is built on:

- **A label is never worth a failed build.** No git on the machine, a source
  tarball with no repository, a checkout that was never initialised — each
  leaves a value empty, and an empty value prints as `unknown`.
- **Two version numbers that disagree stop the build**, which is the one
  deliberate exception. `tauri.conf.json`'s version is the one that ships (an
  Android `versionCode` is derived from it, and dropping the field does not
  fall back to Cargo — the CLI writes no version at all and Gradle ships its
  own default), and Cargo insists on one too. There is no right answer to pick
  between them, so `Stamp` refuses to pick.

A build script does not rerun because you committed, so `Stamp` watches `HEAD`,
the ref it names and `packed-refs` — and only where those exist, since cargo
treats a `rerun-if-changed` on a missing path as *rerun always*. Every value can
also be overridden by an environment variable of its own name, which is how
`build.sh` makes two cargo invocations in one run agree about the time;
`SOURCE_DATE_EPOCH` pins it for a build that has to come out the same twice.

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

`cargo test` (treeserve) and `cargo test -p treesight-shell`. After treeserve
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
