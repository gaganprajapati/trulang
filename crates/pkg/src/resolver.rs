//! Decentralized Package and Module Resolution Engine.
//!
//! Resolves module imports (`import "github.com/org/repo" as alias;`)
//! across local project vendor directories, project-relative paths, and the
//! global cache `~/.trulang/pkg/`.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModule {
    pub name: String,
    pub package_dir: PathBuf,
    pub entry_file: PathBuf,
}

pub struct ModuleResolver;

impl ModuleResolver {
    /// Returns the standard global TruLang package cache directory `~/.trulang/pkg/`.
    pub fn global_cache_dir() -> PathBuf {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".trulang").join("pkg")
    }

    /// Resolves an import string into an on-disk source file.
    pub fn resolve(
        import_path: &str,
        project_dir: Option<&Path>,
    ) -> anyhow::Result<ResolvedModule> {
        // 1. Check relative path imports (e.g. "./utils", "../common/auth.tr")
        if import_path.starts_with("./") || import_path.starts_with("../") {
            if let Some(root) = project_dir {
                let candidate = root.join(import_path);
                if let Some(entry) = find_entry_file(&candidate) {
                    return Ok(ResolvedModule {
                        name: import_path.to_string(),
                        package_dir: candidate,
                        entry_file: entry,
                    });
                }
            }
        }

        // 2. Check project-local packages directory `./packages/<import_path>`
        if let Some(root) = project_dir {
            let local_pkg_dir = root.join("packages").join(import_path);
            if local_pkg_dir.exists() {
                if let Some(entry) = find_entry_file(&local_pkg_dir) {
                    return Ok(ResolvedModule {
                        name: import_path.to_string(),
                        package_dir: local_pkg_dir,
                        entry_file: entry,
                    });
                }
            }
        }

        // 3. Check global cache directory `~/.trulang/pkg/<import_path>`
        let cache_pkg_dir = Self::global_cache_dir().join(import_path);
        if cache_pkg_dir.exists() {
            if let Some(entry) = find_entry_file(&cache_pkg_dir) {
                return Ok(ResolvedModule {
                    name: import_path.to_string(),
                    package_dir: cache_pkg_dir,
                    entry_file: entry,
                });
            }
        }

        anyhow::bail!(
            "Unable to resolve package '{}'. Ensure it is installed via 'trulang add {}' or located in ./packages/",
            import_path,
            import_path
        )
    }
}

/// Locates the entrypoint .tr file for a package directory.
pub fn find_entry_file(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }

    if !path.is_dir() {
        // Try appending .tr
        let with_tr = path.with_extension("tr");
        if with_tr.is_file() {
            return Some(with_tr);
        }
        return None;
    }

    // Standard conventional entrypoints
    let candidates = [
        path.join("src").join("lib.tr"),
        path.join("src").join("main.tr"),
        path.join("lib.tr"),
        path.join("main.tr"),
        path.join(format!(
            "{}.tr",
            path.file_name().unwrap_or_default().to_string_lossy()
        )),
    ];

    for candidate in &candidates {
        if candidate.is_file() {
            return Some(candidate.clone());
        }
    }

    // Check if any .tr file exists in the directory
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map(|e| e == "tr").unwrap_or(false) {
                return Some(p);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_local_package_entry() {
        let temp_dir = std::env::temp_dir().join(format!("trulang_resolve_{}", std::process::id()));
        let pkg_dir = temp_dir.join("packages").join("github.com/trulang/math");
        fs::create_dir_all(pkg_dir.join("src")).expect("Create pkg dir failed");

        let entry = pkg_dir.join("src").join("lib.tr");
        fs::write(&entry, "fn add(a: int, b: int) -> int { return a + b; }")
            .expect("Write lib.tr failed");

        let resolved = ModuleResolver::resolve("github.com/trulang/math", Some(&temp_dir))
            .expect("Resolution failed");

        assert_eq!(resolved.name, "github.com/trulang/math");
        assert_eq!(resolved.entry_file, entry);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
