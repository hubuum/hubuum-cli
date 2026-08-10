use std::collections::{HashMap, HashSet};
use std::fs::{read_dir, read_to_string};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hubuum_extension_protocol::{
    CommandDeclaration, ExtensionManifest, WorkflowDeclaration, MANIFEST_FILENAME,
};
use semver::Version;
use serde_json::{to_value, Value};

use crate::config::AppConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionOrigin {
    System,
    User,
}

impl ExtensionOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionPackState {
    Enabled,
    Disabled,
    Quarantined,
}

impl ExtensionPackState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
            Self::Quarantined => "quarantined",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticSeverity {
    Warning,
    Error,
}

impl DiagnosticSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtensionDiagnostic {
    pub severity: DiagnosticSeverity,
    pub code: &'static str,
    pub pack: Option<String>,
    pub path: PathBuf,
    pub message: String,
}

impl ExtensionDiagnostic {
    fn error(
        code: &'static str,
        pack: Option<String>,
        path: impl Into<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            code,
            pack,
            path: path.into(),
            message: message.into(),
        }
    }

    fn warning(code: &'static str, path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            severity: DiagnosticSeverity::Warning,
            code,
            pack: None,
            path: path.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtensionPack {
    manifest: Option<Arc<ExtensionManifest>>,
    manifest_path: PathBuf,
    package_root: PathBuf,
    executable: Option<PathBuf>,
    origin: ExtensionOrigin,
    state: ExtensionPackState,
    diagnostics: Vec<ExtensionDiagnostic>,
    config: Value,
}

impl ExtensionPack {
    pub fn manifest(&self) -> Option<&ExtensionManifest> {
        self.manifest.as_deref()
    }

    pub fn manifest_arc(&self) -> Option<Arc<ExtensionManifest>> {
        self.manifest.clone()
    }

    pub fn name(&self) -> Option<&str> {
        self.manifest().map(|manifest| manifest.name().as_str())
    }

    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    pub fn package_root(&self) -> &Path {
        &self.package_root
    }

    pub fn executable(&self) -> Option<&Path> {
        self.executable.as_deref()
    }

    pub fn origin(&self) -> ExtensionOrigin {
        self.origin
    }

    pub fn state(&self) -> ExtensionPackState {
        self.state
    }

    pub fn diagnostics(&self) -> &[ExtensionDiagnostic] {
        &self.diagnostics
    }

    pub fn config(&self) -> &Value {
        &self.config
    }

    pub fn is_enabled(&self) -> bool {
        self.state == ExtensionPackState::Enabled
    }

    fn quarantine(&mut self, diagnostic: ExtensionDiagnostic) {
        self.state = ExtensionPackState::Quarantined;
        self.diagnostics.push(diagnostic);
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExtensionRegistry {
    packs: Vec<ExtensionPack>,
    root_diagnostics: Vec<ExtensionDiagnostic>,
}

impl ExtensionRegistry {
    pub fn discover(config: &AppConfig) -> Self {
        let cli_version = Version::parse(env!("CARGO_PKG_VERSION"))
            .expect("Cargo package version should be valid SemVer");
        Self::discover_for_version(config, &cli_version)
    }

    pub fn discover_for_version(config: &AppConfig, cli_version: &Version) -> Self {
        let disabled = config
            .extensions
            .disabled
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let mut registry = Self::default();

        for (origin, roots) in [
            (ExtensionOrigin::System, &config.extensions.system_roots),
            (ExtensionOrigin::User, &config.extensions.user_roots),
        ] {
            for root in roots {
                registry.discover_root(root, origin, config, cli_version, &disabled);
            }
        }

        registry.quarantine_duplicate_names();
        registry.packs.sort_by(|left, right| {
            origin_rank(left.origin)
                .cmp(&origin_rank(right.origin))
                .then_with(|| left.manifest_path.cmp(&right.manifest_path))
        });
        registry
    }

    pub fn packs(&self) -> &[ExtensionPack] {
        &self.packs
    }

    pub fn enabled_packs(&self) -> impl Iterator<Item = &ExtensionPack> {
        self.packs.iter().filter(|pack| pack.is_enabled())
    }

    pub fn pack(&self, name: &str) -> Option<&ExtensionPack> {
        self.packs.iter().find(|pack| pack.name() == Some(name))
    }

    #[cfg(test)]
    pub fn packs_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a ExtensionPack> {
        self.packs
            .iter()
            .filter(move |pack| pack.name() == Some(name))
    }

    pub fn diagnostics(&self) -> Vec<&ExtensionDiagnostic> {
        let mut diagnostics = self.root_diagnostics.iter().collect::<Vec<_>>();
        diagnostics.extend(self.packs.iter().flat_map(|pack| pack.diagnostics.iter()));
        diagnostics.sort_by(|left, right| {
            left.severity
                .cmp(&right.severity)
                .reverse()
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.code.cmp(right.code))
        });
        diagnostics
    }

    pub fn pack_names(&self) -> Vec<String> {
        let mut names = self
            .packs
            .iter()
            .filter_map(|pack| pack.name().map(str::to_string))
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        names
    }

    pub(crate) fn validate_workflows<F>(&mut self, validate: F)
    where
        F: Fn(&CommandDeclaration, &WorkflowDeclaration, &Value) -> Result<(), String>,
    {
        for pack in &mut self.packs {
            if !pack.is_enabled() {
                continue;
            }
            let Some(manifest) = pack.manifest.clone() else {
                continue;
            };
            let invalid = manifest.commands().iter().find_map(|command| {
                command.workflow().and_then(|workflow| {
                    validate(command, workflow, pack.config())
                        .err()
                        .map(|message| {
                            format!(
                                "workflow command '{}' is invalid: {message}",
                                command.path().display()
                            )
                        })
                })
            });
            if let Some(message) = invalid {
                pack.quarantine(ExtensionDiagnostic::error(
                    "workflow_invalid",
                    Some(manifest.name().as_str().to_string()),
                    pack.manifest_path.clone(),
                    message,
                ));
            }
        }
    }

    fn discover_root(
        &mut self,
        root: &Path,
        origin: ExtensionOrigin,
        config: &AppConfig,
        cli_version: &Version,
        disabled: &HashSet<&str>,
    ) {
        let entries = match read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return,
            Err(error) => {
                self.root_diagnostics.push(ExtensionDiagnostic::error(
                    "root_unreadable",
                    None,
                    root,
                    format!("could not read extension root: {error}"),
                ));
                return;
            }
        };

        let mut package_roots = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.root_diagnostics.push(ExtensionDiagnostic::warning(
                        "entry_unreadable",
                        root,
                        format!("could not inspect an extension root entry: {error}"),
                    ));
                    continue;
                }
            };
            let path = entry.path();
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_none_or(|name| name.starts_with('.'))
            {
                continue;
            }
            let metadata = match path.symlink_metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    self.root_diagnostics.push(ExtensionDiagnostic::warning(
                        "entry_unreadable",
                        &path,
                        format!("could not inspect extension root entry: {error}"),
                    ));
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                self.root_diagnostics.push(ExtensionDiagnostic::warning(
                    "symlink_ignored",
                    &path,
                    "extension package symlinks are not followed",
                ));
            } else if metadata.is_dir() {
                package_roots.push(path);
            }
        }
        package_roots.sort();

        for package_root in package_roots {
            self.packs.push(load_pack(
                package_root,
                origin,
                config,
                cli_version,
                disabled,
            ));
        }
    }

    fn quarantine_duplicate_names(&mut self) {
        let mut by_name = HashMap::<String, Vec<usize>>::new();
        for (index, pack) in self.packs.iter().enumerate() {
            if let Some(name) = pack.name() {
                by_name.entry(name.to_string()).or_default().push(index);
            }
        }

        for (name, indices) in by_name.into_iter().filter(|(_, indices)| indices.len() > 1) {
            let sources = indices
                .iter()
                .map(|index| self.packs[*index].manifest_path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            for index in indices {
                let path = self.packs[index].manifest_path.clone();
                self.packs[index].quarantine(ExtensionDiagnostic::error(
                    "duplicate_pack_name",
                    Some(name.clone()),
                    path,
                    format!("pack name '{name}' is declared by multiple manifests: {sources}"),
                ));
            }
        }
    }
}

fn load_pack(
    package_root: PathBuf,
    origin: ExtensionOrigin,
    config: &AppConfig,
    cli_version: &Version,
    disabled: &HashSet<&str>,
) -> ExtensionPack {
    let manifest_path = package_root.join(MANIFEST_FILENAME);
    let mut pack = ExtensionPack {
        manifest: None,
        manifest_path: manifest_path.clone(),
        package_root,
        executable: None,
        origin,
        state: ExtensionPackState::Quarantined,
        diagnostics: Vec::new(),
        config: Value::Object(Default::default()),
    };

    let contents = match read_to_string(&manifest_path) {
        Ok(contents) => contents,
        Err(error) => {
            pack.diagnostics.push(ExtensionDiagnostic::error(
                "manifest_unreadable",
                None,
                &manifest_path,
                format!("could not read manifest: {error}"),
            ));
            return pack;
        }
    };
    let manifest = match ExtensionManifest::parse(&contents) {
        Ok(manifest) => Arc::new(manifest),
        Err(error) => {
            pack.diagnostics.push(ExtensionDiagnostic::error(
                "manifest_invalid",
                None,
                &manifest_path,
                error.to_string(),
            ));
            return pack;
        }
    };
    let name = manifest.name().as_str().to_string();
    pack.config = config
        .extensions
        .config
        .get(&name)
        .and_then(|value| to_value(value).ok())
        .unwrap_or_else(|| Value::Object(Default::default()));

    if !manifest.supports_cli(cli_version) {
        pack.diagnostics.push(ExtensionDiagnostic::error(
            "cli_incompatible",
            Some(name.clone()),
            &manifest_path,
            format!(
                "pack requires CLI {}, but this CLI is {cli_version}",
                manifest.requires_cli()
            ),
        ));
    }

    if let Some(executable_path) = manifest.executable() {
        let executable = pack.package_root.join(executable_path.as_path());
        match executable_status(&executable) {
            Ok(()) => pack.executable = Some(executable),
            Err(message) => pack.diagnostics.push(ExtensionDiagnostic::error(
                "executable_invalid",
                Some(name.clone()),
                &executable,
                message,
            )),
        }
    }

    pack.state = if pack.diagnostics.is_empty() {
        if disabled.contains(name.as_str()) {
            ExtensionPackState::Disabled
        } else {
            ExtensionPackState::Enabled
        }
    } else {
        ExtensionPackState::Quarantined
    };
    pack.manifest = Some(manifest);
    pack
}

fn executable_status(path: &Path) -> Result<(), String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("could not inspect executable '{}': {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("executable '{}' is not a file", path.display()));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(format!("executable '{}' is not executable", path.display()));
        }
    }

    Ok(())
}

fn origin_rank(origin: ExtensionOrigin) -> u8 {
    match origin {
        ExtensionOrigin::System => 0,
        ExtensionOrigin::User => 1,
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{create_dir_all, read_to_string, write};

    use semver::Version;
    use tempfile::tempdir;

    use super::{ExtensionPackState, ExtensionRegistry};
    use crate::config::AppConfig;

    fn write_pack(root: &Path, directory: &str, name: &str) {
        let package = root.join(directory);
        create_dir_all(package.join("bin")).expect("package directories");
        let executable = package.join("bin/extension");
        write(&executable, "#!/bin/sh\nexit 0\n").expect("executable");
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, Permissions::from_mode(0o755))
                .expect("executable permissions");
        }
        write(
            package.join("hubuum-extension.toml"),
            format!(
                r#"schema_version = 1
name = "{name}"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"
executable = "bin/extension"

[[commands]]
path = ["ping"]
"#
            ),
        )
        .expect("manifest");
    }

    use std::path::Path;

    #[test]
    fn discovers_enabled_and_disabled_packs() {
        let directory = tempdir().expect("temporary directory");
        write_pack(directory.path(), "alpha", "alpha");
        write_pack(directory.path(), "beta", "beta");
        let mut config = AppConfig::default();
        config.extensions.system_roots.clear();
        config.extensions.user_roots = vec![directory.path().to_path_buf()];
        config.extensions.disabled = vec!["beta".to_string()];

        let registry = ExtensionRegistry::discover_for_version(&config, &Version::new(0, 0, 9));

        assert_eq!(
            registry.pack("alpha").expect("alpha").state(),
            ExtensionPackState::Enabled
        );
        assert_eq!(
            registry.pack("beta").expect("beta").state(),
            ExtensionPackState::Disabled
        );
    }

    #[test]
    fn quarantines_every_duplicate_pack_name() {
        let directory = tempdir().expect("temporary directory");
        write_pack(directory.path(), "first", "duplicate");
        write_pack(directory.path(), "second", "duplicate");
        let mut config = AppConfig::default();
        config.extensions.system_roots.clear();
        config.extensions.user_roots = vec![directory.path().to_path_buf()];

        let registry = ExtensionRegistry::discover_for_version(&config, &Version::new(0, 0, 9));

        assert_eq!(registry.packs_named("duplicate").count(), 2);
        assert!(registry
            .packs_named("duplicate")
            .all(|pack| pack.state() == ExtensionPackState::Quarantined));
    }

    #[test]
    fn quarantines_missing_executables_with_a_focused_diagnostic() {
        let directory = tempdir().expect("temporary directory");
        write_pack(directory.path(), "missing", "missing");
        std::fs::remove_file(directory.path().join("missing/bin/extension"))
            .expect("remove executable");
        let mut config = AppConfig::default();
        config.extensions.system_roots.clear();
        config.extensions.user_roots = vec![directory.path().to_path_buf()];

        let registry = ExtensionRegistry::discover_for_version(&config, &Version::new(0, 0, 9));
        let pack = registry.pack("missing").expect("missing pack");

        assert_eq!(pack.state(), ExtensionPackState::Quarantined);
        assert_eq!(pack.diagnostics()[0].code, "executable_invalid");
    }

    #[test]
    fn quarantines_incompatible_cli_versions() {
        let directory = tempdir().expect("temporary directory");
        write_pack(directory.path(), "future", "future");
        let manifest = directory.path().join("future/hubuum-extension.toml");
        let contents = read_to_string(&manifest)
            .expect("read manifest")
            .replace(">=0.0.9,<0.1", ">=2.0.0,<3.0.0");
        write(&manifest, contents).expect("update manifest");
        let mut config = AppConfig::default();
        config.extensions.system_roots.clear();
        config.extensions.user_roots = vec![directory.path().to_path_buf()];

        let registry = ExtensionRegistry::discover_for_version(&config, &Version::new(0, 0, 9));
        let pack = registry.pack("future").expect("future pack");

        assert_eq!(pack.state(), ExtensionPackState::Quarantined);
        assert_eq!(pack.diagnostics()[0].code, "cli_incompatible");
    }
}
