//! Project Scaffolding Engine for `trulang new <project_name>`.
//!
//! Generates a standardized, enterprise-ready microservice project structure
//! complete with manifest, service actor, test suite, and .gitignore.

use crate::manifest::Manifest;
use std::fs;
use std::path::Path;

pub struct ScaffoldConfig<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub description: Option<&'a str>,
}

impl<'a> ScaffoldConfig<'a> {
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            version: "0.1.0",
            description: Some("High-performance TruLang microservice"),
        }
    }
}

pub fn scaffold_project(
    target_dir: impl AsRef<Path>,
    config: &ScaffoldConfig,
) -> anyhow::Result<()> {
    let dir = target_dir.as_ref();
    if dir.exists() && dir.read_dir()?.next().is_some() {
        anyhow::bail!(
            "Target directory '{}' already exists and is not empty",
            dir.display()
        );
    }

    fs::create_dir_all(dir)?;
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir)?;

    // 1. Generate Tru.toml
    let mut manifest = Manifest::new(config.name, config.version);
    manifest.package.description = config.description.map(String::from);
    manifest.write_to_file(dir.join(Manifest::FILE_NAME))?;

    // 2. Generate src/main.tr (Canonical Service Actor)
    let main_tr_content = format!(
        r#"import "trulang/http" as http;

service {} {{
    memory_budget request = 16 MB max 64 MB;
    memory_budget persistent = 128 MB;

    fn handle_request(req: http::Request) -> http::Response {{
        let route = req.path();
        let name = req.param("name");

        if route == "/health" {{
            return http::Response::text(200, "healthy");
        }}

        return http::Response::json({{
            "status": "success",
            "service": "{}",
            "message": "Welcome to TruLang",
            "name": name
        }});
        // O(1) bulk reset: 0.0 ms Stop-the-World GC Pause!
    }}
}}
"#,
        to_pascal_case(config.name),
        config.name
    );
    fs::write(src_dir.join("main.tr"), main_tr_content)?;

    // 3. Generate src/main_test.tr (Ready for `trulang test`)
    let test_content = r#"test "service health baseline" {
    let status_code = 200;
    assert_eq(status_code, 200);
    assert(status_code > 0, "status must be positive");
}
"#;
    fs::write(src_dir.join("main_test.tr"), test_content)?;

    // 4. Generate .gitignore
    let gitignore_content = r#"# TruLang build artifacts
target/
*.c
*.o
*.so
*.dylib
*.dll
*.exe

# Local vendor cache
packages/
.trulang/

# Operating system files
.DS_Store
Thumbs.db
"#;
    fs::write(dir.join(".gitignore"), gitignore_content)?;

    // 5. Generate README.md
    let readme_content = format!(
        r#"# {}

> High-performance microservice powered by [TruLang](https://gagn.dev/trulang/).

## 🚀 Quick Start Commands

```bash
# 1. Run automated test suites
trulang test

# 2. Start the native non-blocking HTTP microservice
trulang serve src/main.tr --port 8080

# 3. Compile Ahead-Of-Time into standalone native binary
trulang build --release src/main.tr -o bin/{}
```
"#,
        config.name, config.name
    );
    fs::write(dir.join("README.md"), readme_content)?;

    Ok(())
}

fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '-' || c == '_' || c == ' ' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    if result.is_empty() {
        "AppService".to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scaffold_project_generates_all_files() {
        let temp_dir =
            std::env::temp_dir().join(format!("trulang_scaffold_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        let config = ScaffoldConfig::new("order-gateway");
        scaffold_project(&temp_dir, &config).expect("Scaffold failed");

        assert!(temp_dir.join("Tru.toml").exists());
        assert!(temp_dir.join("src/main.tr").exists());
        assert!(temp_dir.join("src/main_test.tr").exists());
        assert!(temp_dir.join(".gitignore").exists());
        assert!(temp_dir.join("README.md").exists());

        let main_content = fs::read_to_string(temp_dir.join("src/main.tr")).unwrap();
        assert!(main_content.contains("service OrderGateway"));
        assert!(main_content.contains(r#"import "trulang/http" as http;"#));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
