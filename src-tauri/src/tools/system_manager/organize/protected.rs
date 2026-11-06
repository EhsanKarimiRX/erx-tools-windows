//! Protected-path check for the Custom folder target (pure): a chosen
//! folder is REFUSED when it is, or is under, a system location. The real
//! root list is built from the environment in `machine_protected_roots()`;
//! the decision compares the canonical text form case-insensitively, so it
//! is unit-testable with a made-up root list.

use std::path::{Path, PathBuf};

/// One protected location. Exact-only roots (drive roots, the user profile
/// root itself) refuse exactly that path; subtree roots (Windows, Program
/// Files, ProgramData, AppData) refuse the path AND anything under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedRoot {
    pub path: PathBuf,
    pub include_children: bool,
}

impl ProtectedRoot {
    pub fn exact(path: impl Into<PathBuf>) -> Self {
        ProtectedRoot {
            path: path.into(),
            include_children: false,
        }
    }

    pub fn subtree(path: impl Into<PathBuf>) -> Self {
        ProtectedRoot {
            path: path.into(),
            include_children: true,
        }
    }
}

/// True when `chosen` must be refused: it equals a protected root, or (for
/// subtree roots) lives under one. Comparison is text-based and case-
/// insensitive, with a separator boundary so "C:\WindowsSomething" is NOT
/// under "C:\Windows".
pub fn is_protected(chosen: &Path, protected_roots: &[ProtectedRoot]) -> bool {
    let chosen = canonical_text(chosen);
    if chosen.is_empty() {
        return false;
    }
    protected_roots.iter().any(|root| {
        let root_text = canonical_text(&root.path);
        if root_text.is_empty() {
            return false;
        }
        chosen == root_text
            || (root.include_children && chosen.starts_with(&format!("{root_text}\\")))
    })
}

/// Lowercase text of the path: forward slashes unified, the `\\?\`
/// canonicalize prefix stripped, trailing separators dropped. A drive root
/// "C:\" becomes "c:".
fn canonical_text(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    text.trim_end_matches('\\').to_ascii_lowercase()
}

/// Builds the protected root list on this machine: every existing drive
/// root and the user profile root are exact-only; the Windows folder,
/// Program Files (+x86), ProgramData and AppData protect their subtrees.
pub fn machine_protected_roots() -> Vec<ProtectedRoot> {
    let mut roots: Vec<ProtectedRoot> = Vec::new();
    for letter in b'A'..=b'Z' {
        let drive = PathBuf::from(format!("{}:\\", letter as char));
        if drive.is_dir() {
            roots.push(ProtectedRoot::exact(drive));
        }
    }
    for variable in ["windir", "ProgramFiles", "ProgramFiles(x86)", "ProgramData"] {
        if let Ok(value) = std::env::var(variable) {
            roots.push(ProtectedRoot::subtree(value));
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        roots.push(ProtectedRoot::subtree(
            PathBuf::from(&profile).join("AppData"),
        ));
        roots.push(ProtectedRoot::exact(profile));
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<ProtectedRoot> {
        vec![
            ProtectedRoot::exact("C:\\"),
            ProtectedRoot::subtree("C:\\Windows"),
            ProtectedRoot::subtree("C:\\Program Files"),
            ProtectedRoot::subtree("C:\\Program Files (x86)"),
            ProtectedRoot::subtree("C:\\ProgramData"),
            ProtectedRoot::exact("C:\\Users\\erx"),
            ProtectedRoot::subtree("C:\\Users\\erx\\AppData"),
        ]
    }

    #[test]
    fn protected_paths_are_refused() {
        let protected = roots();
        for path in [
            "C:\\",
            "C:\\Windows",
            "C:\\Windows\\Temp\\x",
            "C:\\Program Files",
            "C:\\PROGRAM FILES\\SomeApp",
            "C:\\program files (x86)\\tool",
            "C:\\ProgramData\\secret",
            // The profile root ITSELF is refused…
            "C:\\Users\\erx",
            // …and everything under AppData.
            "C:\\USERS\\ERX\\AppData\\Roaming\\something",
        ] {
            assert!(
                is_protected(Path::new(path), &protected),
                "{path} must be refused"
            );
        }
    }

    #[test]
    fn normal_folders_are_accepted() {
        let protected = roots();
        for path in [
            // Under the profile root is fine — only the root itself is refused.
            "C:\\Users\\erx\\Documents\\misc",
            "C:\\Users\\erx\\Desktop\\projects",
            "C:\\Users\\erx\\Downloads",
            "D:\\data\\inbox",
            "C:\\tools",
        ] {
            assert!(
                !is_protected(Path::new(path), &protected),
                "{path} must be accepted"
            );
        }
    }

    #[test]
    fn other_drive_root_is_also_protected() {
        let protected = vec![ProtectedRoot::exact("C:\\"), ProtectedRoot::exact("D:\\")];
        assert!(is_protected(Path::new("D:\\"), &protected));
        // A drive root protects only itself: D:\stuff is a normal folder.
        assert!(!is_protected(Path::new("D:\\stuff"), &protected));
    }

    #[test]
    fn a_longer_name_is_not_a_root() {
        // C:\WindowsSomething is NOT under C:\Windows (separator boundary).
        assert!(!is_protected(Path::new("C:\\WindowsSomething"), &roots()));
        assert!(!is_protected(
            Path::new("C:\\Users\\erx Extended"),
            &roots()
        ));
    }

    #[test]
    fn machine_roots_include_a_real_drive() {
        let roots = machine_protected_roots();
        assert!(
            roots.iter().any(|root| {
                let text = root.path.to_string_lossy();
                text.len() >= 2 && text.as_bytes()[1] == b':'
            }),
            "at least one drive root, got {:?}",
            roots.iter().map(|root| &root.path).collect::<Vec<_>>()
        );
        assert!(
            roots.iter().any(|root| {
                root.path
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with("appdata")
                    && root.include_children
            }),
            "the AppData folder is a subtree root"
        );
    }

    #[test]
    fn the_real_machine_refuses_its_own_roots() {
        let roots = machine_protected_roots();
        assert!(
            is_protected(Path::new("C:\\"), &roots),
            "the drive root is refused"
        );
        if let Ok(profile) = std::env::var("USERPROFILE") {
            assert!(
                is_protected(Path::new(&profile), &roots),
                "the profile root is refused"
            );
            assert!(
                !is_protected(Path::new(&format!("{profile}\\Documents")), &roots),
                "folders under the profile are fine"
            );
        }
    }
}
