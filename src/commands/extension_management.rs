use std::any::TypeId;
use std::fs::{self, create_dir_all, read_dir, read_to_string, rename};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use hubuum_extension_protocol::{ExtensionManifest, MANIFEST_FILENAME};
use hubuum_filter::OutputEnvelope;
use semver::Version;
use serde_json::{json, to_value, Value};

use crate::catalog::{
    AsyncCommandHandler, CommandCatalogBuilder, CommandContext, CommandEffects, CommandInvocation,
    CommandOutcome, CommandSpec, CompletionSpec, OptionSpec, ScopeAction,
};
use crate::commands::{build_command_catalog, render_format, standard_options, table_headers};
use crate::config::{get_config, reload_runtime_config, set_persisted_value};
use crate::errors::{AppError, ReauthenticationRetry};
use crate::extensions::{ExtensionOrigin, ExtensionPack, ExtensionPackState, WorkflowProgram};
use crate::output::{
    reset_output, set_pipeline, set_pipeline_suffix, set_render_format, set_semantic_output,
    set_table_headers, take_output,
};
use crate::tokenizer::CommandTokenizer;

pub(crate) fn register_commands(builder: &mut CommandCatalogBuilder) {
    let pack_names = builder
        .extensions()
        .pack_names()
        .into_iter()
        .collect::<Vec<_>>();

    register(
        builder,
        "list",
        "List discovered extension packs",
        Vec::new(),
    );
    register(
        builder,
        "show",
        "Show an extension pack and its commands",
        vec![positional(
            "pack",
            "Extension pack name",
            true,
            pack_names.clone(),
        )],
    );
    register(
        builder,
        "doctor",
        "Report extension discovery and validation diagnostics",
        Vec::new(),
    );
    register(
        builder,
        "validate",
        "Validate a local extension package without installing it",
        vec![positional(
            "source",
            "Local package directory",
            true,
            Vec::new(),
        )],
    );
    register(
        builder,
        "explain",
        "Explain a local extension package's compiled workflow plan",
        vec![
            positional("source", "Local package directory", true, Vec::new()),
            named("workflow", "Workflow to explain"),
        ],
    );
    register(
        builder,
        "reload",
        "Reload extension configuration and rebuild the command catalog",
        Vec::new(),
    );
    register(
        builder,
        "install",
        "Install a local extension package into the user extension root",
        vec![
            positional("source", "Local package directory", true, Vec::new()),
            flag("force", None, "Replace an existing user package"),
        ],
    );
    register(
        builder,
        "upgrade",
        "Upgrade an installed user extension from a local package directory",
        vec![
            positional("source", "Local package directory", true, Vec::new()),
            flag("force", None, "Allow a non-increasing version"),
        ],
    );
    for (name, about) in [
        ("enable", "Enable a discovered extension pack"),
        ("disable", "Disable a discovered extension pack"),
        (
            "remove",
            "Move an installed user extension to the extension trash",
        ),
    ] {
        let mut options = vec![positional(
            "pack",
            "Extension pack name",
            true,
            pack_names.clone(),
        )];
        if name == "remove" {
            options.push(flag("force", None, "Remove a quarantined or disabled pack"));
        }
        register(builder, name, about, options);
    }
}

fn register(
    builder: &mut CommandCatalogBuilder,
    name: &'static str,
    about: &'static str,
    mut options: Vec<OptionSpec>,
) {
    options.extend(standard_option_specs());
    let effects = match name {
        "list" | "show" | "doctor" | "validate" | "explain" => CommandEffects::ReadOnly,
        _ => CommandEffects::Mutating,
    };
    let mut spec = CommandSpec::new(
        name,
        options,
        ReauthenticationRetry::Unsafe,
        effects,
        Arc::new(ManagementHandler { operation: name }),
    );
    spec.about = Some(about.to_string());
    spec.examples = management_examples(name);
    builder.add_command(&["extension"], spec);
}

fn management_examples(name: &str) -> Option<String> {
    match name {
        "install" | "upgrade" | "validate" => Some(format!("extension {name} ./my-pack")),
        "explain" => Some("extension explain ./my-pack --workflow snapshot".to_string()),
        "enable" | "disable" | "remove" | "show" => {
            Some(format!("extension {name} site-inventory"))
        }
        "list" | "doctor" | "reload" => Some(format!("extension {name}")),
        _ => None,
    }
}

fn positional(name: &str, help: &str, required: bool, values: Vec<String>) -> OptionSpec {
    OptionSpec {
        name: name.to_string(),
        short: None,
        long: None,
        help: help.to_string(),
        field_type_help: "string".to_string(),
        field_type: TypeId::of::<String>(),
        required,
        flag: false,
        greedy: false,
        nargs: None,
        repeatable: false,
        value_source: false,
        completion: if values.is_empty() {
            CompletionSpec::None
        } else {
            CompletionSpec::Static(values)
        },
    }
}

fn flag(name: &str, short: Option<&str>, help: &str) -> OptionSpec {
    OptionSpec {
        name: name.to_string(),
        short: short.map(str::to_string),
        long: Some(format!("--{name}")),
        help: help.to_string(),
        field_type_help: "flag".to_string(),
        field_type: TypeId::of::<bool>(),
        required: false,
        flag: true,
        greedy: false,
        nargs: None,
        repeatable: false,
        value_source: false,
        completion: CompletionSpec::None,
    }
}

fn named(name: &str, help: &str) -> OptionSpec {
    OptionSpec {
        name: name.to_string(),
        short: None,
        long: Some(format!("--{name}")),
        help: help.to_string(),
        field_type_help: "string".to_string(),
        field_type: TypeId::of::<String>(),
        required: false,
        flag: false,
        greedy: false,
        nargs: None,
        repeatable: false,
        value_source: false,
        completion: CompletionSpec::None,
    }
}

fn standard_option_specs() -> Vec<OptionSpec> {
    standard_options()
        .into_iter()
        .map(|option| OptionSpec {
            name: option.name,
            short: option.short,
            long: option.long,
            help: option.help,
            field_type_help: option.field_type_help,
            field_type: option.field_type,
            required: option.required,
            flag: option.flag,
            greedy: option.greedy,
            nargs: option.nargs,
            repeatable: option.repeatable,
            value_source: option.value_source,
            completion: option
                .autocomplete
                .map(CompletionSpec::Dynamic)
                .unwrap_or(CompletionSpec::None),
        })
        .collect()
}

#[derive(Clone)]
struct ManagementHandler {
    operation: &'static str,
}

#[async_trait]
impl AsyncCommandHandler for ManagementHandler {
    async fn execute(
        &self,
        ctx: CommandContext,
        invocation: CommandInvocation,
    ) -> Result<CommandOutcome, AppError> {
        let tokens = prepare_output(&ctx, &invocation)?;
        match self.operation {
            "list" => list(&ctx)?,
            "show" => show(&ctx, one_positional(&tokens, "pack")?)?,
            "doctor" => doctor(&ctx)?,
            "validate" => validate_command(&ctx, one_positional(&tokens, "source")?)?,
            "explain" => explain_command(
                &ctx,
                one_positional(&tokens, "source")?,
                tokens.get_options().get("workflow").map(String::as_str),
            )?,
            "reload" => reload(&ctx)?,
            "install" => install(&ctx, one_positional(&tokens, "source")?, force(&tokens))?,
            "upgrade" => upgrade(&ctx, one_positional(&tokens, "source")?, force(&tokens))?,
            "enable" => set_enabled(&ctx, one_positional(&tokens, "pack")?, true)?,
            "disable" => set_enabled(&ctx, one_positional(&tokens, "pack")?, false)?,
            "remove" => remove(&ctx, one_positional(&tokens, "pack")?, force(&tokens))?,
            operation => {
                return Err(AppError::CommandExecutionError(format!(
                    "unknown extension management operation '{operation}'"
                )));
            }
        }
        Ok(CommandOutcome {
            output: take_output()?,
            scope_action: ScopeAction::None,
            ..Default::default()
        })
    }

    fn requires_authentication(&self) -> bool {
        false
    }
}

fn prepare_output(
    ctx: &CommandContext,
    invocation: &CommandInvocation,
) -> Result<CommandTokenizer, AppError> {
    reset_output()?;
    set_pipeline(invocation.pipeline.clone())?;
    set_pipeline_suffix(invocation.pipeline_suffix.clone())?;
    let catalog = ctx.catalog.snapshot();
    let resolved = catalog.resolve_command(&[], &invocation.command_path)?;
    let options = resolved
        .command
        .options
        .iter()
        .map(OptionSpec::to_cli_option)
        .collect::<Vec<_>>();
    let tokens = CommandTokenizer::new_without_value_source_resolution_at(
        &invocation.raw_line,
        invocation.command_index,
        &options,
    )?;
    set_render_format(render_format(&tokens)?)?;
    set_table_headers(table_headers(&tokens)?)?;
    Ok(tokens)
}

fn one_positional<'a>(tokens: &'a CommandTokenizer, label: &str) -> Result<&'a str, AppError> {
    match tokens.get_positionals() {
        [value] => Ok(value),
        [] => Err(AppError::ParseError(format!("missing required {label}"))),
        values => Err(AppError::ParseError(format!(
            "expected one {label}, got {}",
            values.len()
        ))),
    }
}

fn force(tokens: &CommandTokenizer) -> bool {
    tokens.get_options().contains_key("force")
}

fn list(ctx: &CommandContext) -> Result<(), AppError> {
    let catalog = ctx.catalog.snapshot();
    let rows = catalog
        .extensions()
        .packs()
        .iter()
        .map(pack_row)
        .collect::<Vec<_>>();
    set_semantic_output(OutputEnvelope::rows(
        rows,
        vec![
            "name".to_string(),
            "version".to_string(),
            "state".to_string(),
            "origin".to_string(),
            "path".to_string(),
        ],
    ))
}

fn pack_row(pack: &ExtensionPack) -> Value {
    json!({
        "name": pack.name(),
        "version": pack.manifest().map(|manifest| manifest.version().to_string()),
        "kind": pack.manifest().map(|manifest| manifest.kind().as_str()),
        "state": pack.state().as_str(),
        "origin": pack.origin().as_str(),
        "path": pack.package_root(),
        "diagnostics": pack.diagnostics().len(),
    })
}

fn show(ctx: &CommandContext, name: &str) -> Result<(), AppError> {
    let catalog = ctx.catalog.snapshot();
    let pack = catalog.extensions().pack(name).ok_or_else(|| {
        AppError::CommandExecutionError(format!("extension pack '{name}' was not discovered"))
    })?;
    let manifest = pack.manifest();
    set_semantic_output(OutputEnvelope::detail(
        json!({
            "name": pack.name(),
            "version": manifest.map(|manifest| manifest.version().to_string()),
            "kind": manifest.map(|manifest| manifest.kind().as_str()),
            "portable": manifest.is_some_and(ExtensionManifest::is_portable),
            "requires_cli": manifest.map(|manifest| manifest.requires_cli().to_string()),
            "protocol": manifest.and_then(ExtensionManifest::protocol).map(|protocol| protocol.as_str()),
            "state": pack.state().as_str(),
            "origin": pack.origin().as_str(),
            "package_root": pack.package_root(),
            "manifest": pack.manifest_path(),
            "executable": pack.executable(),
            "commands": manifest.map(|manifest| manifest.commands().iter().map(|command| {
                json!({
                    "path": command.path().display(),
                    "arguments": command.arguments(),
                    "interactive": command.interactive(),
                    "implementation": if command.workflow().is_some() {
                        "workflow"
                    } else {
                        "executable"
                    },
                    "workflow": command.workflow().map(|workflow| workflow.as_str()),
                })
            }).collect::<Vec<_>>()).unwrap_or_default(),
            "config": manifest.map(|manifest| manifest.config().values().map(|declaration| json!({
                "key": declaration.key(),
                "type": declaration.value_type().as_str(),
                "required": declaration.required(),
                "repeatable": declaration.repeatable(),
                "default": declaration.default(),
                "help": declaration.help(),
            })).collect::<Vec<_>>()).unwrap_or_default(),
            "workflow_plan": pack.workflow_program().map(|program| program.explain_value(None)).transpose()
                .map_err(AppError::CommandExecutionError)?,
            "diagnostics": pack.diagnostics().iter().map(diagnostic_value).collect::<Vec<_>>(),
        }),
        Vec::new(),
    ))
}

fn doctor(ctx: &CommandContext) -> Result<(), AppError> {
    let catalog = ctx.catalog.snapshot();
    let registry = catalog.extensions();
    let diagnostics = registry.diagnostics();
    if diagnostics.is_empty() {
        return set_semantic_output(OutputEnvelope::message(json!({
            "status": "healthy",
            "packs": registry.packs().len(),
            "message": "No extension diagnostics found",
        })));
    }
    set_semantic_output(OutputEnvelope::rows(
        diagnostics.into_iter().map(diagnostic_value).collect(),
        vec![
            "severity".to_string(),
            "code".to_string(),
            "pack".to_string(),
            "path".to_string(),
            "message".to_string(),
        ],
    ))
}

fn validate_command(ctx: &CommandContext, source: &str) -> Result<(), AppError> {
    let source = Path::new(source);
    let (manifest, program) = validate_source_for_catalog(ctx, source)?;
    set_semantic_output(OutputEnvelope::detail(
        json!({
            "status": "valid",
            "source": source,
            "name": manifest.name().as_str(),
            "version": manifest.version().to_string(),
            "kind": manifest.kind().as_str(),
            "portable": manifest.is_portable(),
            "workflow_plan": program.map(|program| program.explain_value(None)).transpose()
                .map_err(AppError::CommandExecutionError)?,
        }),
        Vec::new(),
    ))
}

fn explain_command(
    ctx: &CommandContext,
    source: &str,
    workflow: Option<&str>,
) -> Result<(), AppError> {
    let source = Path::new(source);
    let (manifest, program) = validate_source_for_catalog(ctx, source)?;
    let plan = program
        .map(|program| program.explain_value(workflow))
        .transpose()
        .map_err(AppError::CommandExecutionError)?;
    if workflow.is_some() && plan.is_none() {
        return Err(AppError::CommandExecutionError(
            "--workflow is only valid for portable packs".to_string(),
        ));
    }
    set_semantic_output(OutputEnvelope::detail(
        json!({
            "source": source,
            "name": manifest.name().as_str(),
            "version": manifest.version().to_string(),
            "kind": manifest.kind().as_str(),
            "portable": manifest.is_portable(),
            "protocol": manifest.protocol().map(|protocol| protocol.as_str()),
            "executable": manifest.executable().map(|path| path.as_path()),
            "plan": plan,
        }),
        Vec::new(),
    ))
}

fn diagnostic_value(diagnostic: &crate::extensions::ExtensionDiagnostic) -> Value {
    json!({
        "severity": diagnostic.severity.as_str(),
        "code": diagnostic.code,
        "pack": diagnostic.pack,
        "path": diagnostic.path,
        "message": diagnostic.message,
    })
}

fn reload(ctx: &CommandContext) -> Result<(), AppError> {
    reload_catalog(ctx)?;
    let catalog = ctx.catalog.snapshot();
    set_semantic_output(OutputEnvelope::message(json!({
        "status": "reloaded",
        "packs": catalog.extensions().packs().len(),
        "enabled": catalog.extensions().enabled_packs().count(),
    })))
}

fn set_enabled(ctx: &CommandContext, name: &str, enabled: bool) -> Result<(), AppError> {
    let current = get_config();
    let discovered = ctx.catalog.snapshot();
    let pack = discovered.extensions().pack(name);
    if pack.is_none() && !current.extensions.disabled.iter().any(|item| item == name) {
        return Err(AppError::CommandExecutionError(format!(
            "extension pack '{name}' was not discovered"
        )));
    }
    if enabled && pack.is_some_and(|pack| pack.state() == ExtensionPackState::Quarantined) {
        return Err(AppError::CommandExecutionError(format!(
            "extension pack '{name}' is quarantined; resolve extension doctor diagnostics before enabling it"
        )));
    }

    let mut disabled = current.extensions.disabled.clone();
    disabled.sort();
    disabled.dedup();
    if enabled {
        disabled.retain(|item| item != name);
    } else if !disabled.iter().any(|item| item == name) {
        disabled.push(name.to_string());
        disabled.sort();
    }
    let path = set_persisted_value("extensions.disabled", &disabled.join(","))?;
    reload_catalog(ctx)?;
    let state = ctx
        .catalog
        .snapshot()
        .extensions()
        .pack(name)
        .map(|pack| pack.state().as_str())
        .unwrap_or("not_discovered");
    set_semantic_output(OutputEnvelope::message(json!({
        "name": name,
        "state": state,
        "config": path,
    })))
}

fn install(ctx: &CommandContext, source: &str, force: bool) -> Result<(), AppError> {
    let source = Path::new(source);
    let (manifest, _) = validate_source_for_catalog(ctx, source)?;
    let current = ctx.catalog.snapshot();
    let replace = current
        .extensions()
        .pack(manifest.name().as_str())
        .map(|existing| {
            if !force || existing.origin() != ExtensionOrigin::User {
                return Err(AppError::CommandExecutionError(format!(
                    "extension pack '{}' already exists at {}; use upgrade or install --force for a user pack",
                    manifest.name().as_str(),
                    existing.package_root().display()
                )));
            }
            Ok(existing.package_root().to_path_buf())
        })
        .transpose()?;
    let destination = install_package(source, &manifest, replace.as_deref())?;
    reload_catalog(ctx)?;
    set_semantic_output(OutputEnvelope::message(json!({
        "status": "installed",
        "name": manifest.name().as_str(),
        "version": manifest.version().to_string(),
        "path": destination,
    })))
}

fn upgrade(ctx: &CommandContext, source: &str, force: bool) -> Result<(), AppError> {
    let source = Path::new(source);
    let (manifest, _) = validate_source_for_catalog(ctx, source)?;
    let current = ctx.catalog.snapshot();
    let existing = current
        .extensions()
        .pack(manifest.name().as_str())
        .ok_or_else(|| {
            AppError::CommandExecutionError(format!(
                "extension pack '{}' is not installed; use extension install",
                manifest.name().as_str()
            ))
        })?;
    if existing.origin() != ExtensionOrigin::User {
        return Err(AppError::CommandExecutionError(
            "system extension packs cannot be upgraded by the user command".to_string(),
        ));
    }
    let old_version = existing.manifest().map(|item| item.version().clone());
    if !force
        && old_version
            .as_ref()
            .is_some_and(|old| manifest.version() <= old)
    {
        return Err(AppError::CommandExecutionError(format!(
            "upgrade version {} must be greater than installed version {}; use --force to override",
            manifest.version(),
            old_version.expect("version checked above")
        )));
    }
    let previous_path = existing.package_root().to_path_buf();
    let destination = install_package(source, &manifest, Some(&previous_path))?;
    reload_catalog(ctx)?;
    set_semantic_output(OutputEnvelope::message(json!({
        "status": "upgraded",
        "name": manifest.name().as_str(),
        "from": old_version.map(|version| version.to_string()),
        "to": manifest.version().to_string(),
        "path": destination,
    })))
}

fn remove(ctx: &CommandContext, name: &str, force: bool) -> Result<(), AppError> {
    let current = ctx.catalog.snapshot();
    let pack = current.extensions().pack(name).ok_or_else(|| {
        AppError::CommandExecutionError(format!("extension pack '{name}' was not discovered"))
    })?;
    if pack.origin() != ExtensionOrigin::User {
        return Err(AppError::CommandExecutionError(
            "system extension packs cannot be removed by the user command".to_string(),
        ));
    }
    if pack.state() == ExtensionPackState::Quarantined && !force {
        return Err(AppError::CommandExecutionError(format!(
            "pack '{name}' is {}; pass --force to remove it",
            pack.state().as_str()
        )));
    }
    let version = pack_version(pack);
    let trashed = move_to_trash(pack.package_root(), name, &version)?;
    let mut disabled = get_config().extensions.disabled.clone();
    disabled.retain(|item| item != name);
    let config_path = set_persisted_value("extensions.disabled", &disabled.join(","))?;
    reload_catalog(ctx)?;
    set_semantic_output(OutputEnvelope::message(json!({
        "status": "removed",
        "name": name,
        "trash": trashed,
        "config": config_path,
    })))
}

fn reload_catalog(ctx: &CommandContext) -> Result<(), AppError> {
    reload_runtime_config()?;
    ctx.catalog.replace(build_command_catalog());
    Ok(())
}

fn validate_source(source: &Path) -> Result<ExtensionManifest, AppError> {
    if !source.is_dir() {
        return Err(AppError::CommandExecutionError(format!(
            "extension source '{}' is not a directory",
            source.display()
        )));
    }
    let manifest_path = source.join(MANIFEST_FILENAME);
    let manifest = ExtensionManifest::parse(&read_to_string(&manifest_path)?).map_err(|error| {
        AppError::CommandExecutionError(format!(
            "invalid extension manifest '{}': {error}",
            manifest_path.display()
        ))
    })?;
    let cli_version = Version::parse(env!("CARGO_PKG_VERSION")).expect("valid package version");
    if !manifest.supports_cli(&cli_version) {
        return Err(AppError::CommandExecutionError(format!(
            "extension requires CLI {}, but this CLI is {cli_version}",
            manifest.requires_cli()
        )));
    }
    if let Some(executable) = manifest.executable() {
        validate_executable(&source.join(executable.as_path()))?;
    }
    Ok(manifest)
}

fn validate_source_for_catalog(
    ctx: &CommandContext,
    source: &Path,
) -> Result<(ExtensionManifest, Option<WorkflowProgram>), AppError> {
    let manifest = validate_source(source)?;
    let raw_config = get_config()
        .extensions
        .config
        .get(manifest.name().as_str())
        .and_then(|value| to_value(value).ok())
        .unwrap_or_else(|| Value::Object(Default::default()));
    let config = manifest.resolve_config(&raw_config).map_err(|error| {
        AppError::CommandExecutionError(format!(
            "extension configuration does not satisfy the manifest: {error}"
        ))
    })?;
    let program = if manifest.is_portable() {
        let catalog = ctx.catalog.snapshot();
        Some(
            WorkflowProgram::compile(&manifest, &config, |path| catalog.command(path)).map_err(
                |message| {
                    AppError::CommandExecutionError(format!(
                        "extension workflow compilation failed: {message}"
                    ))
                },
            )?,
        )
    } else {
        None
    };
    Ok((manifest, program))
}

fn validate_executable(path: &Path) -> Result<(), AppError> {
    let metadata = path.metadata().map_err(|error| {
        AppError::CommandExecutionError(format!(
            "could not inspect extension executable '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(AppError::CommandExecutionError(format!(
            "extension executable '{}' is not a file",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(AppError::CommandExecutionError(format!(
                "extension executable '{}' is not executable",
                path.display()
            )));
        }
    }
    Ok(())
}

fn install_package(
    source: &Path,
    manifest: &ExtensionManifest,
    replace: Option<&Path>,
) -> Result<PathBuf, AppError> {
    let config = get_config();
    let root = config.extensions.user_roots.first().ok_or_else(|| {
        AppError::CommandExecutionError("no user extension root is configured".to_string())
    })?;
    create_dir_all(root)?;
    let destination = root.join(manifest.name().as_str());
    if replace.is_some_and(|existing| existing != destination) && destination.exists() {
        return Err(AppError::CommandExecutionError(format!(
            "destination '{}' already contains another package",
            destination.display()
        )));
    }
    if replace.is_none() && destination.exists() {
        return Err(AppError::CommandExecutionError(format!(
            "destination '{}' already exists",
            destination.display()
        )));
    }
    let staging = root.join(format!(
        ".staging-{}-{}",
        manifest.name().as_str(),
        unique_suffix()
    ));
    copy_package(source, &staging).inspect_err(|_| {
        let _ = fs::remove_dir_all(&staging);
    })?;
    validate_source(&staging).inspect_err(|_| {
        let _ = fs::remove_dir_all(&staging);
    })?;

    let backup = if let Some(existing) = replace {
        Some(move_to_trash(
            existing,
            manifest.name().as_str(),
            "previous",
        )?)
    } else if destination.exists() {
        Some(move_to_trash(
            &destination,
            manifest.name().as_str(),
            "replaced",
        )?)
    } else {
        None
    };

    if let Err(error) = rename(&staging, &destination) {
        if let Some(backup) = &backup {
            let _ = rename(backup, &destination);
        }
        return Err(AppError::IoError(error));
    }
    Ok(destination)
}

fn copy_package(source: &Path, destination: &Path) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Err(AppError::CommandExecutionError(format!(
            "extension packages may not contain symlinks: '{}'",
            source.display()
        )));
    }
    if !metadata.is_dir() {
        return Err(AppError::CommandExecutionError(format!(
            "extension package entry '{}' is not a regular directory",
            source.display()
        )));
    }
    create_dir_all(destination)?;
    let mut entries = read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            return Err(AppError::CommandExecutionError(format!(
                "extension packages may not contain symlinks: '{}'",
                source_path.display()
            )));
        }
        if metadata.is_dir() {
            copy_package(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)?;
            fs::set_permissions(&destination_path, metadata.permissions())?;
        } else {
            return Err(AppError::CommandExecutionError(format!(
                "extension packages may contain only regular files and directories: '{}'",
                source_path.display()
            )));
        }
    }
    fs::set_permissions(destination, metadata.permissions())?;
    Ok(())
}

fn move_to_trash(source: &Path, name: &str, version: &str) -> Result<PathBuf, AppError> {
    let parent = source.parent().ok_or_else(|| {
        AppError::CommandExecutionError(format!(
            "extension path '{}' has no parent directory",
            source.display()
        ))
    })?;
    let trash = parent.join(".trash");
    create_dir_all(&trash)?;
    let destination = trash.join(format!("{name}-{version}-{}", unique_suffix()));
    rename(source, &destination)?;
    Ok(destination)
}

fn pack_version(pack: &ExtensionPack) -> String {
    pack.manifest()
        .map(|manifest| manifest.version().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

#[cfg(test)]
mod tests {
    use std::fs::{create_dir_all, write};
    use std::path::Path;

    use tempfile::tempdir;

    use super::{copy_package, validate_source};

    #[test]
    fn validates_and_copies_a_local_package() {
        let temporary = tempdir().expect("temporary directory");
        let source = temporary.path().join("source");
        let destination = temporary.path().join("destination");
        create_dir_all(source.join("bin")).expect("create source");
        let executable = source.join("bin/run");
        write(&executable, "#!/bin/sh\n").expect("write executable");
        #[cfg(unix)]
        {
            use std::fs::Permissions;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, Permissions::from_mode(0o755))
                .expect("executable permissions");
        }
        write(
            source.join("hubuum-extension.toml"),
            r#"schema_version = 1
kind = "executable"
name = "test-pack"
version = "0.1.0"
requires_cli = ">=0.0.9,<0.1"
protocol = "hubuum-cli.extension/v1"
executable = "bin/run"

[[commands]]
path = ["ping"]
"#,
        )
        .expect("write manifest");

        validate_source(&source).expect("valid source");
        copy_package(&source, &destination).expect("copy package");
        validate_source(&destination).expect("valid copied package");
    }

    #[test]
    fn host_pilot_is_a_valid_package() {
        let package = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join("hubuum-wrappers");
        let manifest = validate_source(&package).expect("valid Host pilot package");

        assert_eq!(manifest.name().as_str(), "host");
        assert_eq!(manifest.commands().len(), 3);
    }
}
