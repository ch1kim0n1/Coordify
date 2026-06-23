use coordify_core::paths::{Paths, VERSION};
use coordify_core::{bootstrap, server, session};
use std::os::unix::net::UnixListener;

fn main() {
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
        // Best-effort cleanup so a crash does not strand the lock.
        let _ = std::fs::remove_file(paths.lock());
        let _ = std::fs::remove_file(paths.socket());
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

    println!("coordify-core {VERSION} listening on {}", paths.socket().display());
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
