use std::path::PathBuf;
use std::process::exit;

use treeserve::hl;
use treeserve::page::ThemeMode;
use treeserve::Config;

/// What this build is, from `build.rs`. The header of `--help`, the whole of
/// `--version`, the line the server prints when it starts, and — through
/// `Config` — the footer of every page it serves, so a running server and the
/// binary that started it can never disagree about which build they are.
const BUILD: treestamp::BuildInfo = treestamp::build_info!("TREESERVE_");

fn print_help() {
    print!(
        "\
{label} — serve a directory as a browsable, rendered website

USAGE:
    {name} [OPTIONS] [ROOT]

ARGS:
    ROOT                   directory to serve (default: .)

OPTIONS:
    -b, --bind ADDR        address to bind (default: 127.0.0.1)
    -p, --port PORT        port to listen on (default: 8080)
    -t, --theme MODE       default theme: auto | light | dark (default: auto)
        --no-line-numbers  line numbers off by default
        --no-sidebar       side pane off by default
        --hidden           show dotfiles
        --title NAME       site title (default: root directory name)
        --threads N        worker threads (default: 8)
        --syntax-theme NAME
                           highlighting theme for both light and dark mode
        --syntax-theme-light NAME
                           highlighting theme for light mode (default: InspiredGitHub)
        --syntax-theme-dark NAME
                           highlighting theme for dark mode (default: OneHalfDark)
        --list-syntax-themes
                           list embedded highlighting themes and exit
    -h, --help             print this help
    -V, --version          print the version, the commit and the build time
",
        label = BUILD.label(),
        name = env!("CARGO_PKG_NAME"),
    );
}

fn parse_args() -> Config {
    let mut root: Option<PathBuf> = None;
    let mut title: Option<String> = None;
    let mut bind = "127.0.0.1".to_string();
    let mut port: u16 = 8080;
    let mut theme = ThemeMode::Auto;
    let mut ln = true;
    let mut sidebar = true;
    let mut show_hidden = false;
    let mut threads: usize = 8;
    let mut syn_light = None;
    let mut syn_dark = None;

    let mut args = std::env::args().skip(1);
    let die = |msg: &str| -> ! {
        eprintln!("error: {}", msg);
        eprintln!("run with --help for usage");
        exit(2);
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print_help();
                exit(0);
            }
            "-V" | "--version" => {
                // Several lines, not one: which commit, whether the tree it was
                // cut from was clean, when that commit landed and when this was
                // compiled. On a machine somebody copied a binary to, that is
                // the difference between "it is out of date" and a guess.
                println!("{}", BUILD.report());
                exit(0);
            }
            "-b" | "--bind" => {
                bind = args.next().unwrap_or_else(|| die("--bind needs a value"));
            }
            "-p" | "--port" => {
                port = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| die("--port needs a number"));
            }
            "-t" | "--theme" => {
                let v = args.next().unwrap_or_else(|| die("--theme needs a value"));
                theme = ThemeMode::from_str(&v)
                    .unwrap_or_else(|| die("--theme must be auto, light or dark"));
            }
            "--no-line-numbers" => ln = false,
            "--no-sidebar" => sidebar = false,
            "--hidden" => show_hidden = true,
            "--title" => {
                title = Some(args.next().unwrap_or_else(|| die("--title needs a value")));
            }
            "--threads" => {
                threads = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|&n| n > 0)
                    .unwrap_or_else(|| die("--threads needs a positive number"));
            }
            "--syntax-theme" | "--syntax-theme-light" | "--syntax-theme-dark" => {
                let v = args
                    .next()
                    .unwrap_or_else(|| die(&format!("{} needs a theme name", a)));
                let t = hl::find_theme(&v)
                    .unwrap_or_else(|| die(&format!("unknown theme {:?}; see --list-syntax-themes", v)));
                if a != "--syntax-theme-dark" {
                    syn_light = Some(t);
                }
                if a != "--syntax-theme-light" {
                    syn_dark = Some(t);
                }
            }
            "--list-syntax-themes" => {
                for name in hl::theme_names() {
                    println!("{}", name);
                }
                exit(0);
            }
            _ if a.starts_with('-') => die(&format!("unknown option: {}", a)),
            _ => {
                if root.is_some() {
                    die("multiple ROOT arguments");
                }
                root = Some(PathBuf::from(a));
            }
        }
    }

    let root = root.unwrap_or_else(|| PathBuf::from("."));
    let root = root.canonicalize().unwrap_or_else(|e| {
        eprintln!("error: cannot open root {}: {}", root.display(), e);
        exit(1);
    });
    if !root.is_dir() {
        eprintln!("error: root {} is not a directory", root.display());
        exit(1);
    }

    let mut cfg = Config::new(root);
    // The same stamp the CLI prints, so every page this server writes carries
    // it in the footer. Without this the footer fell back to `CARGO_PKG_*` and
    // said `treeserve v0.1.0` with no commit — the one number that cannot tell
    // two builds apart is the one that never changes between them.
    cfg.app_name = Some(BUILD.name.to_string());
    cfg.app_version = Some(BUILD.version.to_string());
    cfg.app_commit = (!BUILD.commit.is_empty()).then(|| BUILD.commit_mark());
    cfg.set_title(title);
    cfg.bind = bind;
    cfg.port = port;
    cfg.theme = theme;
    cfg.ln = ln;
    cfg.sidebar = sidebar;
    cfg.show_hidden = show_hidden;
    cfg.threads = threads;
    if let Some(t) = syn_light {
        cfg.syn_light = t;
    }
    if let Some(t) = syn_dark {
        cfg.syn_dark = t;
    }
    cfg
}

fn main() {
    let cfg = parse_args();
    let addr = format!("{}:{}", cfg.bind, cfg.port);
    // The CLI always has one: a path from the command line, or the working
    // directory. `rootless` is the shell's state, not this one's.
    let root = cfg.root().map(|r| r.id.clone()).unwrap_or_default();
    let serving = treeserve::spawn(cfg).unwrap_or_else(|e| {
        eprintln!("error: cannot bind {}: {}", addr, e);
        exit(1);
    });
    println!("{}: serving {} at http://{}/", BUILD.label(), root, addr);
    serving.join();
}
