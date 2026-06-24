# Security Policy

Coordify is a **local-first** multi-agent coordination tool. It runs entirely
on your machine: a Rust daemon (`coordify-core`) bound to a Unix domain socket
inside your project's `.coordify/runtime/` directory, plus Node.js hooks and a
CLI/sim that talk to that socket. **Coordify makes zero outbound network
calls** — no telemetry, no analytics, no error reporting, no update checks.

## Supported versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | yes       |

## Threat model

Because there is no cloud surface, the relevant threats are local:

- **Local privilege escalation / token leakage** — the session IPC token gates
  the socket. Coordify mitigates by generating the token from `/dev/urandom`,
  writing it to a `0o600` file inside a `0o700` runtime dir, and checking it on
  every IPC action before any state mutation.
- **Socket hijack** — the socket lives under the project root owned by the
  user, not in a world-writable `/tmp` location.
- **Malicious project roots** — claim/file paths are sanitized: absolute paths
  and `..` traversal are rejected so an agent cannot reference files outside
  the project root.
- **Supply chain** — the dependency tree is minimal (Rust: `serde`, `serde_json`,
  `chrono`; TS: `ink`/`react` for the TUI). `Cargo.lock` and all
  `package-lock.json` files are committed. See the Dependency & CVE checklist.

Coordify assumes a single trusted user on the machine. A second local user with
read access to a world-readable project root could read the session token;
place Coordify projects under a non-world-readable parent directory if that
matters to you.

## File permissions

- `.coordify/runtime/` and `.coordify/sessions/<id>/` are created `0o700`.
- `session.token`, `events.log`, and knowledge files are written `0o600`.

## Data privacy

Coordify is local-first: **all data stays on your machine in `.coordify/`**.
No telemetry, no analytics, no error reporting, no update checks, no cloud.

- **Zero outbound network calls** from Core, hooks, CLI, TUI, or sim. The only
  network code is the local Unix socket between hooks/CLI and Core. Verified by
  dependency audit: no http/https/fetch/analytics SDK in any package.
- **No file contents are ever read or stored by Coordify.** The hook reads only
  the per-session IPC token file. File *paths* from tool events are sent to
  Core and logged; file *contents* (Edit old_string/new_string, Read output)
  are never forwarded — the PreToolUse hooktrace records only the path.
- **No Claude Code transcript contents are copied.** Coordify references
  `transcript_path` in event metadata but never opens or reads the file.
- **User control:** delete all Coordify data with `rm -rf .coordify/`. Run with
  minimal logging by avoiding verbose trace. `.coordify/` is gitignored so
  intelligence does not leak into a public repo.
- **File permissions:** `.coordify/` subdirs are `0o700`; session logs, the
  token, and knowledge files are `0o600`. Other local users cannot read them
  (verified on macOS + Linux; Windows ACL equivalent is tracked under #14).

### Compliance posture (because users may ask)

- **GDPR:** Coordify processes no personal data on Coordify's behalf. The user
  is the controller of any data in `.coordify/`. No Coordify-controlled
  processing = no Coordify GDPR obligation.
- **SCC / cross-border:** N/A — no data leaves the machine.
- **SOC 2 / ISO 27001:** N/A for a local-first MIT tool.
- **Source code confidentiality:** Coordify sees file paths and intents but
  never file contents; safe to run in proprietary codebases.
- **AI training:** Coordify does not send any data to any LLM provider for
  training or otherwise. Claude Code's own behavior is separate.

## Reporting a vulnerability

Please report security issues privately by opening a **draft** GitHub Security
Advisory at https://github.com/ch1kim0n1/Coordify/security/advisories/new, or
email the maintainer. Do not open a public issue for security vulnerabilities.
Include reproduction steps and affected versions. You will receive a response
within 7 days.

## Hardening posture (0.1.0)

- Every IPC action is token-checked before state mutation.
- Unknown actions and unsupported CAP versions return structured errors, never
  panics.
- Malformed JSON on the socket returns an error response and closes the
  connection cleanly.
- `coordify-core` does not exec arbitrary commands from untrusted input; the
  only shell-out is `kill -0` for process-alive checks.
- All filesystem writes are confined to `.coordify/` under the project root.
