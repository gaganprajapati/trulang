//! TruLang Package Manager (tpm) Core Engine.
//!
//! Provides project scaffolding (`trulang new`), dependency management (`trulang add`),
//! cryptographic lockfile verification (`Tru.lock`), and module resolution.

pub mod installer;
pub mod lockfile;
pub mod manifest;
pub mod resolver;
pub mod scaffold;

pub use installer::{AddOptions, PackageInstaller, PublishResult};
pub use lockfile::{compute_directory_sha256, compute_file_sha256, LockedPackage, Lockfile};
pub use manifest::{DependencySpec, Manifest, PackageMetadata};
pub use resolver::{find_entry_file, ModuleResolver, ResolvedModule};
pub use scaffold::{scaffold_project, ScaffoldConfig};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_full_tpm_lifecycle_new_add_install_publish() {
        let temp_dir =
            std::env::temp_dir().join(format!("trulang_tpm_full_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        // 1. trulang new
        let config = ScaffoldConfig::new("payment-service");
        scaffold_project(&temp_dir, &config).expect("Scaffold failed");
        assert!(temp_dir.join("Tru.toml").exists());
        assert!(temp_dir.join("src/main.tr").exists());

        // 2. trulang add
        let locked = PackageInstaller::add(
            &temp_dir,
            "github.com/trulang/postgres",
            AddOptions {
                version: Some("^1.2.0".to_string()),
                git: None,
                path: None,
            },
        )
        .expect("Add failed");

        assert_eq!(locked.name, "github.com/trulang/postgres");
        assert!(temp_dir.join("Tru.lock").exists());

        // 3. trulang install (integrity verification)
        let installed = PackageInstaller::install(&temp_dir).expect("Install failed");
        assert_eq!(installed, 0); // Already up-to-date

        // 4. trulang publish
        let pub_res = PackageInstaller::publish(&temp_dir).expect("Publish failed");
        assert_eq!(pub_res.name, "payment-service");
        assert!(pub_res.file_count >= 1);
        assert!(pub_res.checksum.starts_with("sha256:"));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
