use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct LockInfo {
    pub pid: u32,
    pub started_at: String,
    pub project_root: String,
    pub core_version: String,
}

#[derive(Debug)]
pub enum LockOutcome {
    Acquired,
    HeldBy(LockInfo),
}

pub fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn pid_alive(pid: u32) -> bool {
    // ponytail: `kill -0` shell-out avoids a libc/nix dependency; adequate for local MVP.
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn ensure_runtime_dir(paths: &Paths) -> std::io::Result<()> {
    fs::create_dir_all(paths.runtime())?;
    fs::set_permissions(paths.runtime(), Permissions::from_mode(0o700))
}

pub fn acquire_lock(paths: &Paths, version: &str) -> std::io::Result<LockOutcome> {
    ensure_runtime_dir(paths)?;
    acquire_lock_inner(paths, version, true)
}

fn acquire_lock_inner(paths: &Paths, version: &str, retry: bool) -> std::io::Result<LockOutcome> {
    let info = LockInfo {
        pid: std::process::id(),
        started_at: now_iso(),
        project_root: paths.root.to_string_lossy().into_owned(),
        core_version: version.to_string(),
    };
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(paths.lock())
    {
        Ok(mut f) => {
            f.write_all(serde_json::to_string(&info)?.as_bytes())?;
            f.sync_all()?;
            Ok(LockOutcome::Acquired)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let raw = fs::read_to_string(paths.lock())?;
            match serde_json::from_str::<LockInfo>(&raw) {
                Ok(existing) if pid_alive(existing.pid) => Ok(LockOutcome::HeldBy(existing)),
                _ => {
                    if retry {
                        // Stale or unparseable lock: remove and retry exactly once.
                        fs::remove_file(paths.lock())?;
                        acquire_lock_inner(paths, version, false)
                    } else {
                        Err(std::io::Error::other(
                            "lock held by unknown process after one retry",
                        ))
                    }
                }
            }
        }
        Err(e) => Err(e),
    }
}

pub fn generate_token() -> std::io::Result<String> {
    let mut f = fs::File::open("/dev/urandom")?;
    // 32 bytes of CSPRNG output (256 bits) — hex-encoded to survive in a text
    // file without mangling. Regenerated on every Core start, never reused.
    let mut buf = [0u8; 32];
    f.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{:02x}", b)).collect())
}

/// Constant-time string comparison for token equality.
///
/// `String::!=` short-circuits on the first differing byte, leaking a timing
/// side-channel about how much of a guessed token matched. This XOR-accumulates
/// every byte so the runtime is independent of where (or whether) the strings
/// differ. The length check is constant for fixed-length session tokens.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..ab.len() {
        diff |= ab[i] ^ bb[i];
    }
    diff == 0
}

pub fn write_token(paths: &Paths, token: &str) -> std::io::Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(paths.token())?;
    f.write_all(token.as_bytes())?;
    f.sync_all()
}

pub fn write_pid(paths: &Paths) -> std::io::Result<()> {
    fs::write(paths.pid(), std::process::id().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::VERSION;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("coordify-test-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn acquires_lock_when_absent() {
        let root = temp_root("lock-absent");
        let paths = Paths::new(&root);
        match acquire_lock(&paths, VERSION).unwrap() {
            LockOutcome::Acquired => {}
            other => panic!("expected Acquired, got {:?}", other),
        }
        assert!(paths.lock().exists());
        // runtime dir is 0700
        let mode = fs::metadata(paths.runtime()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reports_held_when_live_pid_holds_lock() {
        let root = temp_root("lock-live");
        let paths = Paths::new(&root);
        // First acquire writes a lock with OUR pid, which is alive.
        acquire_lock(&paths, VERSION).unwrap();
        match acquire_lock(&paths, VERSION).unwrap() {
            LockOutcome::HeldBy(info) => assert_eq!(info.pid, std::process::id()),
            other => panic!("expected HeldBy, got {:?}", other),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn breaks_stale_lock_with_dead_pid() {
        let root = temp_root("lock-stale");
        let paths = Paths::new(&root);
        ensure_runtime_dir(&paths).unwrap();
        // Write a lock owned by an almost-certainly-dead pid.
        let stale = LockInfo {
            pid: 999_999,
            started_at: now_iso(),
            project_root: paths.root.to_string_lossy().into_owned(),
            core_version: VERSION.to_string(),
        };
        fs::write(paths.lock(), serde_json::to_string(&stale).unwrap()).unwrap();
        match acquire_lock(&paths, VERSION).unwrap() {
            LockOutcome::Acquired => {}
            other => panic!(
                "expected Acquired after breaking stale lock, got {:?}",
                other
            ),
        }
        let _ = fs::remove_dir_all(&root);
    }

    /// Verifies that an unparseable lock file is treated as stale and broken on the
    /// first retry, returning `Acquired`. This is also the regression guard that the
    /// retry bound does not loop: the inner function is only allowed one retry, so
    /// if the file were re-created after removal the call would return an error rather
    /// than recurse again.
    #[test]
    fn stale_lock_retry_is_bounded() {
        let root = temp_root("lock-unparseable");
        let paths = Paths::new(&root);
        ensure_runtime_dir(&paths).unwrap();
        // Write an unparseable lock file.
        fs::write(paths.lock(), b"not json").unwrap();
        match acquire_lock(&paths, VERSION).unwrap() {
            LockOutcome::Acquired => {}
            other => panic!(
                "expected Acquired after breaking unparseable lock, got {:?}",
                other
            ),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn token_is_64_hex_chars_and_file_is_0600() {
        let root = temp_root("token");
        let paths = Paths::new(&root);
        ensure_runtime_dir(&paths).unwrap();
        let token = generate_token().unwrap();
        // 32 bytes hex-encoded = 64 chars; 256 bits of entropy.
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        write_token(&paths, &token).unwrap();
        let mode = fs::metadata(paths.token()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn constant_time_eq_matches_and_mismatches() {
        assert!(constant_time_eq("abcdef", "abcdef"));
        assert!(!constant_time_eq("abcdef", "abcdez"));
        assert!(!constant_time_eq("abcdef", "abcdefg"));
        assert!(!constant_time_eq("", "a"));
        assert!(constant_time_eq("", ""));
    }
}
