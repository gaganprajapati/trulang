//! Tru.toml Package Manifest Parser and Serializer.
//!
//! Provides the primary human-readable manifest configuration for TruLang projects.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackageMetadata {
    pub name: String,
    pub version: String,
    pub edition: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum DependencySpec {
    Simple(String),
    Detailed {
        version: Option<String>,
        path: Option<String>,
        git: Option<String>,
        branch: Option<String>,
    },
}

impl DependencySpec {
    pub fn version(&self) -> Option<&str> {
        match self {
            DependencySpec::Simple(v) => Some(v.as_str()),
            DependencySpec::Detailed { version, .. } => version.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub package: PackageMetadata,
    #[serde(default)]
    pub dependencies: BTreeMap<String, DependencySpec>,
}

impl Manifest {
    pub const FILE_NAME: &'static str = "Tru.toml";

    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            package: PackageMetadata {
                name: name.into(),
                version: version.into(),
                edition: "2026".to_string(),
                description: None,
                license: Some("MIT OR Apache-2.0".to_string()),
                authors: Vec::new(),
            },
            dependencies: BTreeMap::new(),
        }
    }

    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let manifest: Self = toml::from_str(content)?;
        Ok(manifest)
    }

    pub fn to_string(&self) -> anyhow::Result<String> {
        let s = toml::to_string_pretty(self)?;
        Ok(s)
    }

    pub fn write_to_file(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let s = self.to_string()?;
        fs::write(path, s)?;
        Ok(())
    }

    pub fn add_dependency(&mut self, name: impl Into<String>, spec: DependencySpec) {
        self.dependencies.insert(name.into(), spec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_prd_tru_toml_manifest() {
        let raw = r#"
[package]
name = "payment-service"
version = "1.0.0"
edition = "2026"

[dependencies]
"github.com/trulang/postgres" = "^1.2.0"
"github.com/trulang/redis" = "^0.8.0"
"#;
        let manifest = Manifest::from_str(raw).expect("Valid Tru.toml");
        assert_eq!(manifest.package.name, "payment-service");
        assert_eq!(manifest.package.version, "1.0.0");
        assert_eq!(manifest.package.edition, "2026");
        assert_eq!(manifest.dependencies.len(), 2);
        assert_eq!(
            manifest.dependencies["github.com/trulang/postgres"],
            DependencySpec::Simple("^1.2.0".to_string())
        );
    }

    #[test]
    fn test_serialize_and_roundtrip_manifest() {
        let mut manifest = Manifest::new("auth-service", "0.2.0");
        manifest.add_dependency(
            "github.com/trulang/jwt",
            DependencySpec::Simple("~0.5.0".to_string()),
        );

        let serialized = manifest.to_string().expect("Serialize success");
        let parsed = Manifest::from_str(&serialized).expect("Roundtrip parse success");
        assert_eq!(manifest, parsed);
    }
}
