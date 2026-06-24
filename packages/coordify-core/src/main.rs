use coordify_core::paths::{Paths, VERSION};
use coordify_core::{bootstrap, server, session};
use std::os::unix::net::UnixListener;

fn main() {
    // Handle --version / -V / --help / -h before any lock acquisition so
    // `coordify-core --version` works even if another Core is running.
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    if raw_args.iter().any(|a| a == "--version" || a == "-V") {
        println!("coordify-core {VERSION}");
        return;
    }
    if raw_args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!(
            "coordify-core {VERSION}\n\
             \n\
             Usage: coordify-core [--root <path>]\n\
             \n\
             Options:\n  \
               --root <path>   Project root (default: current directory)\n  \
               --version, -V   Print version and exit\n  \
               --help, -h      Print this help and exit\n\
             \n\
             Coordify Core is the local runtime that owns canonical live state\n\
             for multi-agent coordination. It binds a Unix domain socket under\n\
             <root>/.coordify/runtime/ and validates CAP events from hooks/CLI.\n\
             \n\
             Platform: macOS, Linux. Windows is not supported in 0.1.0."
        );
        return;
    }

    let root = parse_root();
    let paths = Paths::new(&root);

    match bootstrap::acquire_lock(&paths, VERSION) {
        Ok(bootstrap::LockOutcome::Acquired) => {}
        Ok(bootstrap::LockOutcome::HeldBy(info)) => {
            eprintln!("coordify-core already running (pid {})", info.pid);
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("coordify-core: failed to acquire lock: {e}");
            std::process::exit(1);
        }
    }

    if let Err(e) = run(&paths) {
        eprintln!("coordify-core: {e}");
        // Best-effort cleanup so a crash does not strand the lock or leave a
        // stale token/socket that could be reused or confuse the next start.
        let _ = std::fs::remove_file(paths.lock());
        let _ = std::fs::remove_file(paths.socket());
        let _ = std::fs::remove_file(paths.token());
        let _ = std::fs::remove_file(paths.pid());
        std::process::exit(1);
    }
}

fn run(paths: &Paths) -> std::io::Result<()> {
    let token = bootstrap::generate_token()?;
    bootstrap::write_token(paths, &token)?;
    bootstrap::write_pid(paths)?;

    let sess = session::create_session(paths, session::new_session_id())?;

    // Remove a stale socket file before binding.
    let _ = std::fs::remove_file(paths.socket());
    let listener = UnixListener::bind(paths.socket())?;

    println!(
        "coordify-core {VERSION} listening on {}",
        paths.socket().display()
    );
    server::run(Paths::new(&paths.root), sess, token, listener)
}

fn parse_root() -> std::path::PathBuf {
    // Usage: coordify-core [--root <path>]; defaults to current directory.
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--root" {
            match args.next() {
                Some(p) => return std::path::PathBuf::from(p),
                None => {
                    eprintln!("coordify-core: --root requires a path argument");
                    std::process::exit(1);
                }
            }
        }
    }
    std::path::PathBuf::from(".")
}
