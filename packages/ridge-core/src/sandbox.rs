//! Filesystem sandbox / root-scoping policy (decision **D8 / §5.6**, risk R10).
//!
//! `ridge-core` runs the **same** command implementation behind three hosts.
//! The headless `ridge-cli` host in particular exposes the **whole machine
//! filesystem** to a remote controller — a far larger attack surface than the
//! LAN path, which only ever served a trusted device on the same network
//! (§5.6, §7 R10). Root-scoping is the containment that stops a remote `fs`
//! command from reading `~/.ssh`, `/etc`, or anything outside the workspace
//! roots the host explicitly granted.
//!
//! ## Policy-as-data, injected by the host (backward compatible)
//!
//! The allowed roots are **data** carried on the [`CapabilitySet`] alongside
//! the command whitelist (same D8 "policy is data, not per-host code" idea).
//! They are enforced **once**, at the [`dispatch`](crate::dispatch::dispatch)
//! entry, next to the existing `..` traversal guard.
//!
//! **Empty roots = unrestricted.** When the host injects no roots the sandbox
//! is a no-op, which is exactly the current desktop / LAN behaviour — so adding
//! this layer cannot break the in-process desktop IPC path or the trusted-LAN
//! path. A host turns the sandbox *on* by handing `dispatch` a `CapabilitySet`
//! with one or more roots (the cloud headless host injects the workspace
//! root(s); the desktop host may leave it empty for now). See
//! [`RootScope`].
//!
//! ## What this guards, and the symlink boundary (honest limits)
//!
//! The check is **lexical**: a path-bearing argument is normalised by resolving
//! `.` / `..` segments *without touching the filesystem* (so it works for paths
//! that do not exist yet — `create_file`, `write_file` targets — and is
//! deterministic in tests), then tested for containment under some allowed
//! root. Lexical normalisation already neutralises `..` escape attempts that
//! survive the literal `..` guard (e.g. an absolute path that simply sits
//! outside every root).
//!
//! Lexical normalisation **cannot** follow symlinks — resolving those needs
//! `std::fs::canonicalize`, which requires the path to exist and performs FS
//! I/O. As a best-effort hardening we *additionally* canonicalize when the
//! target (or its nearest existing ancestor) exists and re-check containment,
//! so a symlink inside a root that points outside it is caught. A symlink that
//! does not yet resolve cannot be checked at admission time; defence-in-depth
//! against symlink escape on *write* targets is left to the host's own FS layer
//! and is noted for S4/S5. This boundary is documented rather than hidden.

use std::path::{Component, Path, PathBuf};

/// The workspace-root sandbox policy: the set of directories under which
/// path-bearing commands are permitted to operate.
///
/// Held as data on the [`CapabilitySet`](crate::capability::CapabilitySet) and
/// consulted once at dispatch. **An empty scope means "unrestricted"** — the
/// backward-compatible default that preserves current desktop / LAN behaviour.
#[derive(Debug, Clone, Default)]
pub struct RootScope {
    /// Lexically-normalised, absolute allowed roots. Empty = unrestricted.
    roots: Vec<PathBuf>,
}

impl RootScope {
    /// An unrestricted scope (no roots). Equivalent to today's desktop / LAN
    /// behaviour: every path is allowed. This is the [`Default`].
    pub fn unrestricted() -> Self {
        Self { roots: Vec::new() }
    }

    /// Build a scope from a list of workspace roots. Each root is lexically
    /// normalised (so `..` / `.` inside a configured root is resolved up front).
    ///
    /// Relative roots are kept as-is after lexical normalisation; hosts are
    /// expected to inject **absolute** workspace roots. A root that normalises
    /// to empty is dropped.
    pub fn from_roots<I, P>(roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let roots = roots
            .into_iter()
            .map(|p| lexically_normalize(p.as_ref()))
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
        Self { roots }
    }

    /// True if no roots are configured (the unrestricted, backward-compatible
    /// default).
    pub fn is_unrestricted(&self) -> bool {
        self.roots.is_empty()
    }

    /// The configured roots (lexically normalised). Empty ⇒ unrestricted.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// True if `candidate` is permitted: i.e. the scope is unrestricted (no
    /// roots) or the candidate resolves inside some allowed root.
    ///
    /// Containment is decided on the lexically normalised path; when the
    /// candidate (or its nearest existing ancestor) resolves on disk, a
    /// best-effort `canonicalize` re-check additionally rejects a path that is
    /// lexically inside a root but symlinks *out* of it. The dispatch guard maps
    /// a `false` here to
    /// [`CoreError::OutsideSandbox`](crate::error::CoreError::OutsideSandbox).
    pub fn is_allowed(&self, candidate: &str) -> bool {
        if self.roots.is_empty() {
            return true; // unrestricted (backward compatible)
        }
        let normalized = lexically_normalize(Path::new(candidate));
        if !self.contains_lexically(&normalized) {
            return false;
        }
        // Best-effort: if it resolves on disk, make sure the *canonical* form
        // (symlinks followed) is still inside a root. A path that does not yet
        // resolve passes on its lexical containment alone.
        match best_effort_canonical(&normalized) {
            Some(real) => self.contains_canonical(&real),
            None => true,
        }
    }

    /// Lexical containment: is `path` equal to, or a descendant of, a root?
    fn contains_lexically(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| is_within(path, root))
    }

    /// Canonical containment: canonicalize each root (best effort) and test.
    /// A root that cannot be canonicalized falls back to the lexical form so a
    /// non-existent-but-configured root never spuriously rejects everything.
    fn contains_canonical(&self, real: &Path) -> bool {
        self.roots.iter().any(|root| {
            let root_real = best_effort_canonical(root).unwrap_or_else(|| root.clone());
            is_within(real, &root_real)
        })
    }
}

/// True if `path` is `base` itself or lives underneath it (component-wise, so
/// `/work/app2` is NOT "within" `/work/app`).
fn is_within(path: &Path, base: &Path) -> bool {
    path.starts_with(base)
}

/// Resolve `.` and `..` segments **lexically** — no filesystem access, so it
/// works for paths that do not exist yet and never blocks on I/O.
///
/// `..` pops the last real segment; a `..` that would escape past the path's
/// root/prefix is dropped (it cannot go above the root lexically). Mixed
/// separators are handled by `Path`'s component iterator.
fn lexically_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {} // drop `.`
            Component::ParentDir => {
                // Pop a normal segment if we have one; otherwise keep climbing
                // only when the accumulated path is still relative (so a
                // leading `..` in a relative path is preserved, but `..` can
                // never rise above an absolute root/prefix).
                if !pop_normal(&mut out) && !out.has_root() && out.as_os_str().is_empty() {
                    out.push("..");
                }
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                out.push(comp.as_os_str());
            }
        }
    }
    out
}

/// Pop the trailing component iff it is a `Normal` segment (not a root/prefix
/// and not itself a `..`). Returns whether a pop happened.
fn pop_normal(out: &mut PathBuf) -> bool {
    let last_is_normal = out
        .components()
        .next_back()
        .map(|c| matches!(c, Component::Normal(_)))
        .unwrap_or(false);
    if last_is_normal {
        out.pop();
        true
    } else {
        false
    }
}

/// Canonicalize `path`, or its nearest existing ancestor with the non-existent
/// tail re-appended, so we get a symlink-resolved form even for a not-yet-
/// created target. Returns `None` if nothing along the chain exists.
fn best_effort_canonical(path: &Path) -> Option<PathBuf> {
    if let Ok(real) = std::fs::canonicalize(path) {
        return Some(real);
    }
    // Walk up to the nearest existing ancestor, canonicalize it, then re-attach
    // the remaining (non-existent) tail lexically.
    let mut ancestor = path.parent();
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    if let Some(name) = path.file_name() {
        tail.push(name);
    }
    while let Some(anc) = ancestor {
        if let Ok(real) = std::fs::canonicalize(anc) {
            let mut result = real;
            for seg in tail.iter().rev() {
                result.push(seg);
            }
            return Some(result);
        }
        if let Some(name) = anc.file_name() {
            tail.push(name);
        }
        ancestor = anc.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrestricted_allows_anything() {
        let scope = RootScope::unrestricted();
        assert!(scope.is_unrestricted());
        assert!(scope.is_allowed("/etc/passwd"));
        assert!(scope.is_allowed("anything/at/all"));
        // Default is unrestricted.
        assert!(RootScope::default().is_allowed("/etc/shadow"));
    }

    #[test]
    fn path_inside_root_is_allowed() {
        let scope = RootScope::from_roots(["/work/project"]);
        assert!(scope.is_allowed("/work/project/src/main.rs"));
        assert!(scope.is_allowed("/work/project")); // the root itself
    }

    #[test]
    fn path_outside_root_is_rejected() {
        let scope = RootScope::from_roots(["/work/project"]);
        assert!(!scope.is_allowed("/etc/passwd"));
        assert!(!scope.is_allowed("/work/other"));
        // Sibling whose name is a prefix of the root must NOT match.
        let scope2 = RootScope::from_roots(["/work/app"]);
        assert!(!scope2.is_allowed("/work/app2/secret"));
    }

    #[test]
    fn dotdot_escape_to_outside_root_is_rejected() {
        let scope = RootScope::from_roots(["/work/project"]);
        // Lexically resolves to /work/secret — outside the root.
        assert!(!scope.is_allowed("/work/project/../secret"));
        // …and one that climbs all the way out.
        assert!(!scope.is_allowed("/work/project/../../etc/passwd"));
    }

    #[test]
    fn dotdot_within_root_is_allowed() {
        let scope = RootScope::from_roots(["/work/project"]);
        // /work/project/a/../b normalises to /work/project/b — still inside.
        assert!(scope.is_allowed("/work/project/a/../b"));
    }

    #[test]
    fn multiple_roots_each_admit_their_own() {
        let scope = RootScope::from_roots(["/work/a", "/srv/b"]);
        assert!(scope.is_allowed("/work/a/x"));
        assert!(scope.is_allowed("/srv/b/y"));
        assert!(!scope.is_allowed("/work/c/z"));
    }

    #[test]
    fn lexical_normalize_resolves_dot_and_dotdot() {
        assert_eq!(
            lexically_normalize(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        // Relative leading `..` is preserved (cannot climb above an unknown base).
        assert_eq!(
            lexically_normalize(Path::new("../x/y")),
            PathBuf::from("../x/y")
        );
        // `..` cannot rise above an absolute root.
        assert_eq!(
            lexically_normalize(Path::new("/../../x")),
            PathBuf::from("/x")
        );
    }

    #[test]
    fn symlink_escape_is_caught_when_resolvable() {
        // Build: <tmp>/root/link -> <tmp>/outside ; root is the only allowed root.
        // A path *through* the symlink lexically looks inside but canonically
        // escapes — must be rejected. Skipped where symlink creation is denied
        // (e.g. unprivileged Windows without Developer Mode).
        let base = std::env::temp_dir().join(format!(
            "ridge-core-sandbox-symlink-{}-{}",
            std::process::id(),
            line!()
        ));
        let root = base.join("root");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"x").unwrap();

        let link = root.join("link");
        let made = make_dir_symlink(&outside, &link);

        let scope = RootScope::from_roots([root.to_string_lossy().into_owned()]);
        if made {
            let through_link = link.join("secret.txt");
            // Lexically inside `root`, but resolves outside ⇒ rejected.
            assert!(
                !scope.is_allowed(&through_link.to_string_lossy()),
                "symlink escape should be rejected"
            );
        }
        // A genuine file directly inside the root is always allowed.
        std::fs::write(root.join("ok.txt"), b"y").unwrap();
        assert!(scope.is_allowed(&root.join("ok.txt").to_string_lossy()));

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Create a directory symlink cross-platform; returns false if unsupported
    /// / not permitted so the test can soft-skip the symlink assertion.
    fn make_dir_symlink(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_dir(target, link).is_ok()
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, link);
            false
        }
    }
}
