#!/bin/sh
# Build the two binaries into dist/, and optionally install them.
#
#   treeserve  - the HTTP server and CLI. Pure Rust, one file, no runtime deps.
#   treesight  - the desktop app. One file too, but it loads the platform
#                webview at run time: WebKitGTK on Linux (so build it on the
#                distro you will run it on), WKWebView on macOS, WebView2 on
#                Windows.
#
# Neither binary reads anything from disk beyond the folder you point it at:
# stylesheets, all syntax themes, the Markdown parser, the Mermaid renderer
# and the LaTeX renderer are compiled in, and nothing is fetched at run time.
#
# Usage:
#   ./build.sh                    server + app  -> dist/
#   ./build.sh server             server only   -> dist/treeserve
#   ./build.sh static             server, fully static (musl, no libc at all)
#   ./build.sh windows            Windows .exe, cross-compiled -> dist/windows/
#   ./build.sh install [DIR]      copy dist/* to DIR, default ~/bin
#   ./build.sh bundle             server + app + installers (needs cargo-tauri)
set -eu

target=${1:-all}
# install destination: argument, then $PREFIX, then ~/bin.
prefix=${2:-${PREFIX:-$HOME/bin}}
cd "$(dirname "$0")"

# What this run actually produced. The summary at the end used to list dist/
# instead, which is not the same question: a Linux build would report the .exe
# files that a Windows build had left there days earlier, in the one place
# anybody looks to check what they just got.
built=""
# What the summary at the end says this build was cut from; filled by `stamp`.
stamp_line=""

# What the compile bakes into the binary: which commit, and when. Computed once
# here and exported, because one run of this script invokes cargo more than
# once and two artifacts disagreeing about when they were built would be two
# answers to the one question a stamp exists to answer.
#
# **Every value may be empty**, and none of them may stop a build. `stamp/` asks
# git itself when one is absent — which is the bare `cargo build` path — and
# prints "unknown" when git has nothing to say either. No git on the machine and
# a source tarball with no repository both build, and say so rather than
# claiming a commit.
say_git() {
    dir=$1
    shift
    [ -d "$dir" ] || return 0
    (cd "$dir" && git "$@" 2>/dev/null) || true
}

stamp() {
    TREESIGHT_COMMIT=$(say_git . rev-parse --short=8 HEAD)
    # The committer date, not the author date: the author date survives a
    # rebase, so on a rebased branch it can predate the code in the artifact.
    TREESIGHT_COMMIT_TIME=$(say_git . log -1 --format=%cI)
    TREESIGHT_BRANCH=$(say_git . rev-parse --abbrev-ref HEAD)
    # Tracked files differing from HEAD, as a count — and only where there is a
    # HEAD to differ from, because `grep -c` on no input says 0, and 0 would
    # claim a clean tree that was never looked at.
    TREESIGHT_DIRTY=""
    [ -z "$TREESIGHT_COMMIT" ] ||
        TREESIGHT_DIRTY=$(say_git . diff --name-only HEAD | grep -c . || true)
    # SOURCE_DATE_EPOCH is the reproducible-builds way of saying "pretend it is
    # this moment", and `stamp/` already knows how to turn one into a timestamp.
    # Left empty so that it gets the chance: `date -u -d @…` is a GNU spelling,
    # and this script runs on macOS too.
    TREESIGHT_BUILD_TIME=""
    [ -n "${SOURCE_DATE_EPOCH:-}" ] ||
        TREESIGHT_BUILD_TIME=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    export TREESIGHT_COMMIT TREESIGHT_COMMIT_TIME TREESIGHT_BRANCH \
        TREESIGHT_DIRTY TREESIGHT_BUILD_TIME

    stamp_line=${TREESIGHT_COMMIT:-no git}
    case ${TREESIGHT_DIRTY:-0} in
    0) ;;
    *) stamp_line="$stamp_line+$TREESIGHT_DIRTY (uncommitted)" ;;
    esac
    [ -z "$TREESIGHT_BUILD_TIME" ] ||
        stamp_line="$stamp_line, $TREESIGHT_BUILD_TIME"
}

# Copies a freshly built file into dist/ and records that we made it.
keep() {
    name=$(basename "$1")
    # Unlinked first rather than written through: copying over a binary that is
    # running fails with ETXTBSY, and "you may not rebuild the app while the app
    # is open" is not a rule this script has any business enforcing — least of
    # all after the compile, having spent the minutes and thrown them away. The
    # process that is running keeps the inode it started from and never notices.
    rm -f "$2/$name"
    cp "$1" "$2/$name"
    built="$built $2/$name"
}

build_server() {
    echo "[build] treeserve (server + CLI)"
    cargo build --release
    mkdir -p dist
    keep target/release/treeserve dist
}

build_app() {
    echo "[build] treesight (desktop app)"
    cargo build --release -p treesight
    mkdir -p dist
    keep target/release/treesight dist
}

# Cross-compiles both .exe files from Linux. zig is the whole Windows
# toolchain here — it ships a mingw-w64 sysroot, a C compiler, a linker and a
# resource compiler, so this needs no distro packages and no root. Everything
# else is a translation layer: cargo-zigbuild points cargo at zig, and two
# shims answer for the binutils names that cargo and tauri-winres still
# spell out. Installers are Windows-only; this produces bare binaries.
build_windows() {
    triple=x86_64-pc-windows-gnu
    if ! rustup target list --installed 2>/dev/null | grep -qx "$triple"; then
        echo "error: target $triple is not installed. Add it with:" >&2
        echo "    rustup target add $triple" >&2
        exit 1
    fi
    if ! command -v zig >/dev/null 2>&1; then
        echo "error: zig not found. Unpack a release from https://ziglang.org/download/" >&2
        echo "       and put it on your PATH; 0.15 or newer works." >&2
        exit 1
    fi
    if ! command -v cargo-zigbuild >/dev/null 2>&1; then
        echo "error: cargo-zigbuild not found. Install it with one of:" >&2
        echo "    cargo install cargo-zigbuild --locked" >&2
        echo "    cargo binstall cargo-zigbuild" >&2
        exit 1
    fi

    shims=target/zig-shims
    mkdir -p "$shims"
    # rustc shells out to dlltool to build import libraries for the
    # raw-dylib imports in windows-sys.
    cat >"$shims/dlltool" <<'EOF'
#!/bin/sh
exec zig dlltool "$@"
EOF
    # The icon, the version info and the DPI-awareness manifest go through
    # tauri-winres, which will only drive a GNU windres. zig rc does the same
    # job — including its own preprocessing against the mingw headers — so the
    # shim claims that identity when probed and translates the arguments.
    cat >"$shims/windres" <<'EOF'
#!/bin/sh
case " $* " in *" -V "*|*" --version "*) echo "GNU windres (zig rc)"; exit 0 ;; esac
in=; out=; args=""
while [ $# -gt 0 ]; do
    case $1 in
    --input|-i) in=$2; shift 2 ;;
    --input=*) in=${1#*=}; shift ;;
    --output|-o) out=$2; shift 2 ;;
    --output=*) out=${1#*=}; shift ;;
    --include-dir|-I) args="$args /i '$2'"; shift 2 ;;
    --include-dir=*) args="$args /i '${1#*=}'"; shift ;;
    -D) args="$args /d '$2'"; shift 2 ;;
    -D*) args="$args /d '${1#-D}'"; shift ;;
    *) shift ;;
    esac
done
eval exec zig rc /:output-format coff /:target x86_64 /:auto-includes gnu \
    $args /fo "'$out'" -- "'$in'"
EOF
    chmod +x "$shims/dlltool" "$shims/windres"

    # This build prints "ignoring deprecated linker optimization setting '1'"
    # once per binary, and it is not ours: rustc passes `-Wl,-O1` when it
    # optimises, zig's linker answers that the setting is deprecated and ignored,
    # and rustc's `linker_messages` lint repeats the answer. Two tools discussing
    # a flag neither we nor they chose, reporting that the linker did *less*
    # rather than that anything failed. Silencing it means turning off the lint
    # that would show a real linker error, so it stays.
    echo "[build] treeserve.exe + treesight.exe ($triple)"
    RUSTFLAGS="${RUSTFLAGS:-} -C dlltool=$PWD/$shims/dlltool" \
        RC_x86_64_pc_windows_gnu="$PWD/$shims/windres" \
        cargo zigbuild --release --target "$triple" -p treeserve -p treesight
    mkdir -p dist/windows
    keep "target/$triple/release/treeserve.exe" dist/windows
    keep "target/$triple/release/treesight.exe" dist/windows
    # treeserve.exe is one self-contained file. treesight.exe is not: this
    # target links Microsoft's WebView2 loader as a DLL rather than the static
    # library the MSVC build gets, so that one file has to travel with it.
    # It is the copy the crate vendors, which is what we just linked against.
    loader=$(ls -t "target/$triple"/release/build/webview2-com-sys-*/out/x64/WebView2Loader.dll 2>/dev/null | head -1)
    if [ -z "$loader" ]; then
        echo "error: WebView2Loader.dll not found under target/$triple/release/build/" >&2
        exit 1
    fi
    keep "$loader" dist/windows
}

stamp

case $target in
all)
    build_server
    build_app
    ;;
server)
    build_server
    ;;
static)
    # musl + the pure-Rust highlighting engine: no libc, no C dependency, so
    # the result runs on any Linux of this architecture. Highlighting is about
    # twice as slow as the default build; output is identical.
    triple="$(uname -m)-unknown-linux-musl"
    if ! rustup target list --installed 2>/dev/null | grep -qx "$triple"; then
        echo "error: target $triple is not installed. Add it with:" >&2
        echo "    rustup target add $triple" >&2
        exit 1
    fi
    echo "[build] treeserve, static ($triple)"
    cargo build --release --target "$triple" --no-default-features --features pure
    mkdir -p dist
    keep "target/$triple/release/treeserve" dist
    ;;
windows)
    build_windows
    ;;
install)
    [ -x dist/treeserve ] || build_server
    mkdir -p "$prefix"
    for f in dist/*; do
        [ -f "$f" ] || continue
        cp "$f" "$prefix/"
        echo "[install] $prefix/$(basename "$f")"
    done
    case ":$PATH:" in
    *":$prefix:"*) ;;
    *) echo "note: $prefix is not on your PATH" ;;
    esac
    exit 0
    ;;
bundle)
    build_server
    build_app
    if ! command -v cargo-tauri >/dev/null 2>&1; then
        echo "error: cargo-tauri not found. Install it with one of:" >&2
        echo "    cargo install tauri-cli --version '^2' --locked" >&2
        echo "    cargo binstall tauri-cli" >&2
        exit 1
    fi
    echo "[build] installers"
    # Fetches the NSIS/AppImage tooling on first run; output lands in
    # target/release/bundle/.
    (cd app && cargo tauri build)
    ;;
*)
    echo "usage: $0 [all|server|static|windows|bundle]" >&2
    echo "       $0 install [DIR]        default: $HOME/bin" >&2
    exit 2
    ;;
esac

echo
echo "built:"
for f in $built; do
    echo "    $f  ($(wc -c <"$f" | tr -d ' ') bytes)"
done
# Which commit is inside what was just written. The binaries say the same thing
# to `--version` and in the page footer; this is so the answer is on screen when
# the files appear, rather than only once they are installed somewhere.
[ -z "$built" ] || echo "    from $stamp_line"
[ "$target" = bundle ] && echo "    target/release/bundle/ (installers)"
# The .exe files are nobody's business but `$0 windows`, and they sit in dist/
# looking exactly like the ones we just made. Anyone reading this list is
# reading it to find out what is now current, so say which part of it is not.
if [ "$target" != windows ] && [ -f dist/windows/treesight.exe ]; then
    echo
    echo "note: dist/windows/ is from an earlier '$0 windows' and was NOT rebuilt"
fi
echo
if [ "$target" = windows ]; then
    echo "copy dist/windows/ to the Windows machine; treesight.exe needs"
    echo "WebView2Loader.dll in the same folder, treeserve.exe needs nothing."
else
    echo "install with: $0 install [DIR]    (default $prefix)"
fi
exit 0
