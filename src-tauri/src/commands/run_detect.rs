//! Best-effort auto-detection of a project's dev/run command, used to
//! prefill the Run tab's command field before the user has set an explicit
//! override (`ProjectSettings::run_command`). Checked in order: a
//! `package.json` `dev`/`start` script, a `Cargo.toml` (`cargo run`), then a
//! `pyproject.toml` script entry. First match wins; nothing here actually
//! runs any of these commands, so a wrong guess is just a wrong prefill the
//! user can edit before clicking Run.

use std::path::Path;

/// Best-effort guess at this project's dev/run command, or `None` if
/// nothing recognizable was found. See module docs for the detection order.
pub(crate) fn detect_run_command(cwd: &Path) -> Option<String> {
    detect_from_package_json(cwd)
        .or_else(|| detect_from_cargo_toml(cwd))
        .or_else(|| detect_from_pyproject(cwd))
}

enum PackageManager {
    Pnpm,
    Yarn,
    Npm,
}

fn package_manager(cwd: &Path) -> PackageManager {
    if cwd.join("pnpm-lock.yaml").is_file() {
        PackageManager::Pnpm
    } else if cwd.join("yarn.lock").is_file() {
        PackageManager::Yarn
    } else {
        PackageManager::Npm
    }
}

fn detect_from_package_json(cwd: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(cwd.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&contents).ok()?;
    let scripts = value.get("scripts")?.as_object()?;
    let script = if scripts.contains_key("dev") {
        "dev"
    } else if scripts.contains_key("start") {
        "start"
    } else {
        return None;
    };
    Some(match package_manager(cwd) {
        PackageManager::Pnpm => format!("pnpm run {script}"),
        PackageManager::Yarn => format!("yarn {script}"),
        PackageManager::Npm => format!("npm run {script}"),
    })
}

fn detect_from_cargo_toml(cwd: &Path) -> Option<String> {
    cwd.join("Cargo.toml")
        .is_file()
        .then(|| "cargo run".to_string())
}

/// Naive section scan, not a real TOML parser: looks for a
/// `[project.scripts]` or `[tool.poetry.scripts]` table and a `dev`/`start`
/// key inside it. Returns the key itself as the command (e.g. `dev`) — this
/// assumes the script is already installed on `PATH` (e.g. via `pip install
/// -e .` or `poetry install`), which corral has no way to verify; a wrong
/// guess is just a wrong prefill the user can overwrite.
fn detect_from_pyproject(cwd: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(cwd.join("pyproject.toml")).ok()?;
    let mut in_scripts_section = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_scripts_section =
                trimmed == "[project.scripts]" || trimmed == "[tool.poetry.scripts]";
            continue;
        }
        if !in_scripts_section {
            continue;
        }
        let Some((key, _)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key == "dev" || key == "start" {
            return Some(key.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn detects_pnpm_dev_script_with_pnpm_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "vite"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(
            detect_run_command(dir.path()),
            Some("pnpm run dev".to_string())
        );
    }

    #[test]
    fn falls_back_to_start_script_when_no_dev_script() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"start": "node server.js"}}"#,
        )
        .unwrap();
        assert_eq!(
            detect_run_command(dir.path()),
            Some("npm run start".to_string())
        );
    }

    #[test]
    fn uses_yarn_when_yarn_lock_present() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "next dev"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("yarn.lock"), "").unwrap();
        assert_eq!(detect_run_command(dir.path()), Some("yarn dev".to_string()));
    }

    #[test]
    fn falls_back_to_cargo_run_when_no_package_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"").unwrap();
        assert_eq!(
            detect_run_command(dir.path()),
            Some("cargo run".to_string())
        );
    }

    #[test]
    fn falls_back_to_pyproject_scripts_entry() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nname = \"x\"\n\n[project.scripts]\ndev = \"x.cli:main\"\n",
        )
        .unwrap();
        assert_eq!(detect_run_command(dir.path()), Some("dev".to_string()));
    }

    #[test]
    fn package_json_without_a_dev_or_start_script_falls_through_to_cargo() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"build": "tsc"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"").unwrap();
        assert_eq!(
            detect_run_command(dir.path()),
            Some("cargo run".to_string())
        );
    }

    #[test]
    fn returns_none_when_nothing_recognizable_exists() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_run_command(dir.path()), None);
    }
}
