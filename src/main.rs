use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const UPSTREAM_RAW: &str =
    "https://raw.githubusercontent.com/walkinglabs/learn-harness-engineering/main";
const PACK_NAME: &str = "openai-advanced";

#[derive(Parser)]
#[command(name = "codex-harness")]
#[command(version)]
#[command(about = "Set up and audit Codex-ready harness docs for a repository")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Doctor,
    Source(SourceCommand),
    Repo(RepoCommand),
    Audit(TargetArgs),
    Validate(TargetArgs),
    Sop(SopCommand),
    Request(RequestCommand),
}

#[derive(Args)]
struct SourceCommand {
    #[command(subcommand)]
    command: SourceCommands,
}

#[derive(Subcommand)]
enum SourceCommands {
    List,
    Files(PackArgs),
    Get(SourceGetArgs),
}

#[derive(Args)]
struct PackArgs {
    #[arg(long, default_value = PACK_NAME)]
    pack: String,
}

#[derive(Args)]
struct SourceGetArgs {
    path: String,
}

#[derive(Args)]
struct RepoCommand {
    #[command(subcommand)]
    command: RepoCommands,
}

#[derive(Subcommand)]
enum RepoCommands {
    Inspect(TargetArgs),
    Plan(RepoPlanArgs),
    Apply(RepoApplyArgs),
}

#[derive(Args, Clone)]
struct TargetArgs {
    #[arg(long, default_value = ".")]
    target: PathBuf,
}

#[derive(Args)]
struct RepoPlanArgs {
    #[arg(long, default_value = ".")]
    target: PathBuf,
    #[arg(long, default_value = PACK_NAME)]
    pack: String,
    #[arg(long, value_enum, default_value_t = PlanMode::WorkNotes)]
    plan_mode: PlanMode,
}

#[derive(Args)]
struct RepoApplyArgs {
    #[arg(long, default_value = ".")]
    target: PathBuf,
    #[arg(long, default_value = PACK_NAME)]
    pack: String,
    #[arg(long, value_enum, default_value_t = PlanMode::WorkNotes)]
    plan_mode: PlanMode,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, value_enum, default_value_t = ApplyMode::FillMissing)]
    mode: ApplyMode,
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct SopCommand {
    #[command(subcommand)]
    command: SopCommands,
}

#[derive(Subcommand)]
enum SopCommands {
    List,
    Show { name: String },
}

#[derive(Args)]
struct RequestCommand {
    #[command(subcommand)]
    command: RequestCommands,
}

#[derive(Subcommand)]
enum RequestCommands {
    Get { path: String },
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PlanMode {
    WorkNotes,
    Tracked,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, ValueEnum)]
enum ApplyMode {
    FillMissing,
}

#[derive(Serialize)]
struct Envelope<T: Serialize> {
    ok: bool,
    command: String,
    data: T,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    ok: bool,
    command: String,
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: String,
    message: String,
    hint: Option<String>,
}

#[derive(Clone, Serialize)]
struct TemplateFile {
    path: &'static str,
    content: String,
}

#[derive(Serialize)]
struct FilePlan {
    path: String,
    action: String,
    reason: String,
}

#[derive(Serialize)]
struct RepoInspection {
    target: String,
    exists: bool,
    has_agents: bool,
    has_architecture: bool,
    has_docs: bool,
    has_quality_score: bool,
    has_reliability: bool,
    has_security: bool,
    has_work_notes_default: bool,
    template_check: TemplateCheck,
    existing_harness_files: Vec<String>,
}

#[derive(Serialize)]
struct ApplyResult {
    target: String,
    pack: String,
    plan_mode: PlanMode,
    dry_run: bool,
    created: Vec<String>,
    skipped: Vec<String>,
    conflicts: Vec<String>,
    next: Vec<String>,
}

#[derive(Serialize)]
struct CommandRun {
    command: Vec<String>,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    template_check: Option<TemplateCheck>,
}

#[derive(Clone, Serialize)]
struct TemplateCheck {
    ok: bool,
    stale_templates: Vec<String>,
    findings: Vec<TemplateFinding>,
    message: String,
}

#[derive(Clone, Serialize)]
struct TemplateFinding {
    path: String,
    reason: String,
}

struct CommandOutcome {
    value: Value,
    should_fail: bool,
}

fn main() {
    let cli = Cli::parse();
    let command_name = command_name(&cli.command);
    let result = run(&cli, command_name);

    match result {
        Ok(outcome) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&Envelope {
                        ok: !outcome.should_fail,
                        command: command_name.to_string(),
                        data: outcome.value,
                        warnings: vec![],
                    })
                    .expect("serialize success envelope")
                );
            } else {
                print_human(&outcome.value);
            }
            if outcome.should_fail {
                std::process::exit(1);
            }
        }
        Err(error) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&ErrorEnvelope {
                        ok: false,
                        command: command_name.to_string(),
                        error: ErrorBody {
                            code: "command_failed".to_string(),
                            message: error.to_string(),
                            hint: Some("Run with --help for command usage.".to_string()),
                        },
                    })
                    .expect("serialize error envelope")
                );
            } else {
                eprintln!("error: {error:#}");
            }
            std::process::exit(1);
        }
    }
}

fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Doctor => "doctor",
        Commands::Source(_) => "source",
        Commands::Repo(_) => "repo",
        Commands::Audit(_) => "audit",
        Commands::Validate(_) => "validate",
        Commands::Sop(_) => "sop",
        Commands::Request(_) => "request",
    }
}

fn run(cli: &Cli, command_name: &str) -> Result<CommandOutcome> {
    let value = match &cli.command {
        Commands::Doctor => doctor()?,
        Commands::Source(source) => run_source(source)?,
        Commands::Repo(repo) => run_repo(repo)?,
        Commands::Audit(args) => run_external(
            command_name,
            vec![
                codex_home()?.join("audit-harness.sh"),
                absolutize(&args.target)?,
            ],
            Some(&args.target),
        )?,
        Commands::Validate(args) => run_external(
            command_name,
            vec![
                PathBuf::from("node"),
                PathBuf::from(
                    "/Users/kmsh/.agents/skills/harness-creator/scripts/validate-harness.mjs",
                ),
                PathBuf::from("--target"),
                absolutize(&args.target)?,
            ],
            Some(&args.target),
        )?,
        Commands::Sop(sop) => run_sop(sop)?,
        Commands::Request(request) => run_request(request, cli.json)?,
    };
    let should_fail = audit_command_should_fail(&cli.command, &value);
    Ok(CommandOutcome { value, should_fail })
}

fn audit_command_should_fail(command: &Commands, value: &Value) -> bool {
    matches!(command, Commands::Audit(_) | Commands::Validate(_))
        && value.get("exit_code").and_then(Value::as_i64) != Some(0)
}

fn print_human(value: &Value) {
    if let Some(text) = value.get("message").and_then(Value::as_str) {
        println!("{text}");
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(value).expect("serialize human output")
        );
    }
}

fn doctor() -> Result<Value> {
    let codex_home = codex_home()?;
    let audit = codex_home.join("audit-harness.sh");
    let skill = PathBuf::from("/Users/kmsh/.agents/skills/harness-creator/SKILL.md");
    let validator =
        PathBuf::from("/Users/kmsh/.agents/skills/harness-creator/scripts/validate-harness.mjs");

    Ok(json!({
        "version": VERSION,
        "pack": PACK_NAME,
        "plan_mode_default": "work-notes",
        "paths": {
            "codex_home": codex_home,
            "audit_harness": audit,
            "harness_creator_skill": skill,
            "harness_creator_validator": validator
        },
        "checks": {
            "audit_harness_installed": audit.is_file(),
            "audit_harness_executable": is_executable(&audit),
            "harness_creator_installed": skill.is_file(),
            "harness_creator_validator_installed": validator.is_file(),
            "node_available": command_exists("node"),
            "cargo_available": command_exists("cargo")
        },
        "message": "codex-harness doctor complete"
    }))
}

fn run_source(source: &SourceCommand) -> Result<Value> {
    match &source.command {
        SourceCommands::List => Ok(json!({
            "packs": [{
                "name": PACK_NAME,
                "source": "walkinglabs/learn-harness-engineering/docs/en/resources/openai-advanced",
                "default_plan_mode": "work-notes",
                "template_file_count": template_files(PlanMode::WorkNotes).len()
            }]
        })),
        SourceCommands::Files(args) => {
            ensure_pack(&args.pack)?;
            Ok(json!({
                "pack": args.pack,
                "files": template_files(PlanMode::WorkNotes)
                    .into_iter()
                    .map(|file| file.path)
                    .collect::<Vec<_>>()
            }))
        }
        SourceCommands::Get(args) => {
            let content = get_template_or_sop(&args.path)
                .ok_or_else(|| anyhow!("unknown bundled source path: {}", args.path))?;
            Ok(json!({ "path": args.path, "content": content }))
        }
    }
}

fn run_repo(repo: &RepoCommand) -> Result<Value> {
    match &repo.command {
        RepoCommands::Inspect(args) => Ok(serde_json::to_value(inspect_repo(&args.target)?)?),
        RepoCommands::Plan(args) => {
            ensure_pack(&args.pack)?;
            Ok(json!({
                "target": absolutize(&args.target)?,
                "pack": args.pack,
                "plan_mode": args.plan_mode,
                "files": plan_files(&args.target, args.plan_mode)?
            }))
        }
        RepoCommands::Apply(args) => {
            ensure_pack(&args.pack)?;
            if args.force {
                bail!(
                    "--force is reserved for a future explicit overwrite mode; current apply is fill-missing only"
                );
            }
            Ok(serde_json::to_value(apply_files(args)?)?)
        }
    }
}

fn run_sop(sop: &SopCommand) -> Result<Value> {
    match &sop.command {
        SopCommands::List => Ok(json!({
            "sops": SOP_FILES.iter().map(|(name, _, summary)| {
                json!({ "name": name, "summary": summary })
            }).collect::<Vec<_>>()
        })),
        SopCommands::Show { name } => {
            let (_, content, summary) = SOP_FILES
                .iter()
                .find(|(sop_name, _, _)| *sop_name == name)
                .ok_or_else(|| anyhow!("unknown SOP: {name}"))?;
            Ok(json!({ "name": name, "summary": summary, "content": content }))
        }
    }
}

fn run_request(request: &RequestCommand, json_output: bool) -> Result<Value> {
    match &request.command {
        RequestCommands::Get { path } => {
            let safe_path = sanitize_request_path(path)?;
            let url = format!("{UPSTREAM_RAW}/{safe_path}");
            let body = reqwest::blocking::get(&url)
                .with_context(|| format!("GET {url}"))?
                .error_for_status()
                .with_context(|| format!("GET {url} returned an error"))?
                .text()
                .with_context(|| format!("read body from {url}"))?;
            if json_output {
                Ok(json!({ "method": "GET", "url": url, "body": body }))
            } else {
                Ok(json!({ "message": body }))
            }
        }
    }
}

fn sanitize_request_path(path: &str) -> Result<String> {
    let safe_path = path.trim_start_matches('/');
    if safe_path.contains("..") {
        bail!("request path must not contain '..'");
    }
    Ok(safe_path.to_string())
}

fn run_external(command_name: &str, parts: Vec<PathBuf>, target: Option<&Path>) -> Result<Value> {
    let (program, args) = parts
        .split_first()
        .ok_or_else(|| anyhow!("missing external command"))?;
    let mut command = Command::new(program);
    for arg in args {
        command.arg(arg);
    }
    let output = command
        .output()
        .with_context(|| format!("run external {command_name} command"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let template_check = target
        .map(check_for_stale_templates)
        .transpose()
        .with_context(|| format!("check templates after {command_name}"))?;
    let exit_code = if template_check
        .as_ref()
        .map(|check| check.ok)
        .unwrap_or(true)
    {
        output.status.code()
    } else {
        Some(output.status.code().unwrap_or(1).max(1))
    };
    Ok(serde_json::to_value(CommandRun {
        command: parts
            .into_iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect(),
        exit_code,
        stdout,
        stderr,
        template_check,
    })?)
}

fn check_for_stale_templates(target: &Path) -> Result<TemplateCheck> {
    let target = absolutize(target)?;
    let mut findings_by_path = BTreeMap::new();
    let mut seen_paths = BTreeSet::new();
    for file in template_files(PlanMode::WorkNotes)
        .into_iter()
        .chain(template_files(PlanMode::Tracked))
    {
        if !seen_paths.insert(file.path) {
            continue;
        }
        let existing = match fs::read_to_string(target.join(file.path)) {
            Ok(existing) => existing,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => bail!("read {}: {error}", file.path),
        };
        let reason = if existing == file.content {
            Some("matches bundled starter template exactly")
        } else if contains_unresolved_placeholder(&existing) {
            Some("contains unresolved starter placeholder text")
        } else {
            None
        };
        if let Some(reason) = reason {
            findings_by_path.insert(
                file.path.to_string(),
                TemplateFinding {
                    path: file.path.to_string(),
                    reason: reason.to_string(),
                },
            );
        }
    }

    let findings = findings_by_path.into_values().collect::<Vec<_>>();
    let stale_templates = findings
        .iter()
        .map(|finding| finding.path.clone())
        .collect::<Vec<_>>();
    let ok = findings.is_empty();
    Ok(TemplateCheck {
        ok,
        stale_templates,
        findings,
        message: if ok {
            "No exact bundled starter templates found.".to_string()
        } else {
            "Harness docs still contain starter templates or unresolved placeholders; replace them with repo-specific facts, promote durable rules, or remove unused starter files."
                .to_string()
        },
    })
}

fn contains_unresolved_placeholder(content: &str) -> bool {
    content.contains("[`")
        || content.contains("`]")
        || content.contains("[describe")
        || content.contains("[command")
        || content.contains("[source of truth]")
        || content.contains("[primary user]")
        || content.contains("[rule]")
        || content.contains("[area]")
        || content.contains("[test, audit, review, incident]")
        || content.contains("[when to revisit]")
        || content.contains("[log, endpoint, UI state, metric]")
        || content.contains("[flow]")
        || content.contains("[signal]")
        || content.contains("[step]")
        || content.contains("[check]")
}

fn inspect_repo(target: &Path) -> Result<RepoInspection> {
    let target = absolutize(target)?;
    let docs = target.join("docs");
    let mut existing_harness_files = vec![];
    for path in [
        "AGENTS.md",
        "ARCHITECTURE.md",
        "docs/PLANS.md",
        "docs/QUALITY_SCORE.md",
        "docs/RELIABILITY.md",
        "docs/SECURITY.md",
    ] {
        if target.join(path).exists() {
            existing_harness_files.push(path.to_string());
        }
    }

    Ok(RepoInspection {
        target: target.to_string_lossy().to_string(),
        exists: target.exists(),
        has_agents: target.join("AGENTS.md").is_file(),
        has_architecture: target.join("ARCHITECTURE.md").is_file(),
        has_docs: docs.is_dir(),
        has_quality_score: target.join("docs/QUALITY_SCORE.md").is_file(),
        has_reliability: target.join("docs/RELIABILITY.md").is_file(),
        has_security: target.join("docs/SECURITY.md").is_file(),
        has_work_notes_default: has_work_notes_text(&target),
        template_check: check_for_stale_templates(&target)?,
        existing_harness_files,
    })
}

fn plan_files(target: &Path, plan_mode: PlanMode) -> Result<Vec<FilePlan>> {
    let target = absolutize(target)?;
    let mut plans = vec![];
    for file in template_files(plan_mode) {
        let destination = target.join(file.path);
        let action = if !destination.exists() {
            "create"
        } else {
            let existing = fs::read_to_string(&destination).unwrap_or_default();
            if existing == file.content {
                "skip"
            } else {
                "conflict"
            }
        };
        let reason = match action {
            "create" => "file is missing",
            "skip" => "file already matches template",
            _ => "file exists with different content",
        };
        plans.push(FilePlan {
            path: file.path.to_string(),
            action: action.to_string(),
            reason: reason.to_string(),
        });
    }
    Ok(plans)
}

fn apply_files(args: &RepoApplyArgs) -> Result<ApplyResult> {
    let target = absolutize(&args.target)?;
    if !target.exists() {
        bail!("target does not exist: {}", target.display());
    }
    let mut created = vec![];
    let mut skipped = vec![];
    let mut conflicts = vec![];

    for file in template_files(args.plan_mode) {
        let destination = target.join(file.path);
        if destination.exists() {
            let existing = fs::read_to_string(&destination).unwrap_or_default();
            if existing == file.content {
                skipped.push(file.path.to_string());
            } else {
                conflicts.push(file.path.to_string());
            }
            continue;
        }

        created.push(file.path.to_string());
        if !args.dry_run {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create directory {}", parent.display()))?;
            }
            let mut handle = fs::File::create(&destination)
                .with_context(|| format!("create {}", destination.display()))?;
            handle
                .write_all(file.content.as_bytes())
                .with_context(|| format!("write {}", destination.display()))?;
        }
    }

    Ok(ApplyResult {
        target: target.to_string_lossy().to_string(),
        pack: PACK_NAME.to_string(),
        plan_mode: args.plan_mode,
        dry_run: args.dry_run,
        created,
        skipped,
        conflicts,
        next: vec![
            "Review created placeholders and replace sample text with repo-specific facts."
                .to_string(),
            "Run codex-harness --json audit --target <repo>.".to_string(),
            "Run codex-harness --json validate --target <repo>.".to_string(),
        ],
    })
}

fn template_files(plan_mode: PlanMode) -> Vec<TemplateFile> {
    let plan_policy = match plan_mode {
        PlanMode::WorkNotes => work_notes_plans_md(),
        PlanMode::Tracked => tracked_plans_md(),
    };
    let agents = match plan_mode {
        PlanMode::WorkNotes => agents_work_notes(),
        PlanMode::Tracked => agents_tracked(),
    };

    let mut files = vec![
        TemplateFile {
            path: "AGENTS.md",
            content: agents,
        },
        TemplateFile {
            path: "ARCHITECTURE.md",
            content: architecture_md(),
        },
        TemplateFile {
            path: "docs/PLANS.md",
            content: plan_policy,
        },
        TemplateFile {
            path: "docs/DESIGN.md",
            content: design_md(),
        },
        TemplateFile {
            path: "docs/FRONTEND.md",
            content: frontend_md(),
        },
        TemplateFile {
            path: "docs/PRODUCT_SENSE.md",
            content: product_sense_md(),
        },
        TemplateFile {
            path: "docs/QUALITY_SCORE.md",
            content: quality_score_md(),
        },
        TemplateFile {
            path: "docs/RELIABILITY.md",
            content: reliability_md(),
        },
        TemplateFile {
            path: "docs/SECURITY.md",
            content: security_md(plan_mode),
        },
        TemplateFile {
            path: "docs/design-docs/index.md",
            content: design_docs_index_md(),
        },
        TemplateFile {
            path: "docs/design-docs/core-beliefs.md",
            content: core_beliefs_md(),
        },
        TemplateFile {
            path: "docs/generated/db-schema.md",
            content: generated_db_schema_md(),
        },
        TemplateFile {
            path: "docs/product-specs/index.md",
            content: product_specs_index_md(),
        },
        TemplateFile {
            path: "docs/product-specs/new-user-onboarding.md",
            content: new_user_onboarding_md(),
        },
        TemplateFile {
            path: "docs/references/design-system-reference-llms.txt",
            content: design_system_ref(),
        },
        TemplateFile {
            path: "docs/references/nixpacks-llms.txt",
            content: nixpacks_ref(),
        },
        TemplateFile {
            path: "docs/references/uv-llms.txt",
            content: uv_ref(),
        },
    ];

    if plan_mode == PlanMode::Tracked {
        files.push(TemplateFile {
            path: "docs/exec-plans/active/index.md",
            content: active_plans_index_md(),
        });
        files.push(TemplateFile {
            path: "docs/exec-plans/completed/index.md",
            content: completed_plans_index_md(),
        });
        files.push(TemplateFile {
            path: "docs/exec-plans/tech-debt-tracker.md",
            content: tech_debt_tracker_md(),
        });
    } else {
        files.push(TemplateFile {
            path: "docs/TECH_DEBT.md",
            content: tech_debt_tracker_md(),
        });
    }

    files
}

fn get_template_or_sop(path: &str) -> Option<String> {
    template_files(PlanMode::WorkNotes)
        .into_iter()
        .find(|file| file.path == path)
        .map(|file| file.content)
        .or_else(|| {
            SOP_FILES
                .iter()
                .find(|(name, _, _)| *name == path || format!("{name}.md") == path)
                .map(|(_, content, _)| content.to_string())
        })
}

fn ensure_pack(pack: &str) -> Result<()> {
    if pack != PACK_NAME {
        bail!("unknown pack: {pack}; supported pack: {PACK_NAME}");
    }
    Ok(())
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn codex_home() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("CODEX_HOME") {
        return Ok(PathBuf::from(home));
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow!("could not determine home directory"))?;
    Ok(home.join(".codex"))
}

fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn has_work_notes_text(target: &Path) -> bool {
    WalkDir::new(target)
        .max_depth(3)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| {
            matches!(
                entry.path().extension().and_then(|ext| ext.to_str()),
                Some("md")
            )
        })
        .any(|entry| {
            fs::read_to_string(entry.path())
                .map(|text| text.contains("$CODEX_HOME/work-notes"))
                .unwrap_or(false)
        })
}

static SOP_FILES: &[(&str, &str, &str)] = &[
    (
        "encode-knowledge-into-repo",
        include_str!("sops/encode-knowledge-into-repo.md"),
        "Move agent-invisible knowledge into repo-local docs.",
    ),
    (
        "layered-domain-architecture",
        include_str!("sops/layered-domain-architecture.md"),
        "Establish explicit layers and cross-cutting boundaries.",
    ),
    (
        "observability-feedback-loop",
        include_str!("sops/observability-feedback-loop.md"),
        "Give agents logs, metrics, traces, and a repeatable debug loop.",
    ),
    (
        "chrome-devtools-validation-loop",
        include_str!("sops/chrome-devtools-validation-loop.md"),
        "Use browser automation and snapshots to validate UI behavior.",
    ),
];

fn agents_work_notes() -> String {
    r#"# AGENTS.md

This repository is optimized for long-running Codex work. Keep this file short.
Use it as the routing layer into system-of-record docs, not as an encyclopedia.

## Startup Workflow

Before changing code:

1. Confirm the repo root with `pwd`.
2. Read `ARCHITECTURE.md` for the current system map and hard dependency rules.
3. Read `docs/QUALITY_SCORE.md` to see which domains or layers are weakest.
4. Read `docs/PLANS.md`, then open the active scratch note under `$CODEX_HOME/work-notes/<repo-slug>/<branch-or-thread>/` when one governs the work.
5. Read the relevant product spec in `docs/product-specs/`.
6. Run the standard bootstrap and verification path for this repo.
7. If baseline verification is failing, repair the baseline before adding scope.

## Routing Map

- `ARCHITECTURE.md`: domain map, layer model, dependency rules
- `docs/design-docs/index.md`: design decisions and core beliefs
- `docs/product-specs/index.md`: current product behaviors and acceptance targets
- `docs/PLANS.md`: scratch-plan lifecycle and promotion policy
- `docs/QUALITY_SCORE.md`: product-domain and layer health
- `docs/RELIABILITY.md`: runtime signals, benchmarks, and restart expectations
- `docs/SECURITY.md`: secrets, sandbox, data, and external-action rules
- `docs/FRONTEND.md`: UI constraints, design system rules, accessibility checks
- `docs/TECH_DEBT.md`: durable deferred work that survives a branch

## Working Contract

- Work from one bounded plan or feature slice at a time.
- Store scratch specs, plans, notes, and execution logs under `$CODEX_HOME/work-notes/<repo-slug>/<branch-or-thread>/` unless the user explicitly asks for tracked docs.
- Do not mark work done from code inspection alone; runnable evidence is required.
- If you change behavior, update the matching product, plan, or reliability docs in the same session.
- If you see repeated review feedback, promote it into a mechanical rule, check, or linter instead of re-explaining it in chat.
- Keep generated material in `docs/generated/` and source references in `docs/references/`.
- Prefer adding small, current docs over growing this file.

## Definition Of Done

A change is done only when all of the following are true:

- target behavior is implemented
- required verification actually ran
- sanitized evidence is linked from the relevant scratch note, quality document, or runbook
- affected docs remain current
- the repository can restart cleanly from the standard startup path

## End Of Session

Before ending a session:

1. Update the active scratch note under `$CODEX_HOME/work-notes/<repo-slug>/<branch-or-thread>/`.
2. Promote only durable rules, decisions, commands, and evidence into tracked docs.
3. Update `docs/QUALITY_SCORE.md` if any domain or layer meaningfully changed.
4. Record durable deferred work in `docs/TECH_DEBT.md` if you intentionally deferred it.
5. Mark scratch notes done, superseded, or abandoned.
6. Leave the repo in a restartable state with a clear next action.
"#
    .to_string()
}

fn agents_tracked() -> String {
    r#"# AGENTS.md

This repository is optimized for long-running coding-agent work. Keep this file
short. Use it as the routing layer into the system-of-record docs, not as a
giant instruction dump.

## Startup Workflow

Before changing code:

1. Confirm the repo root with `pwd`.
2. Read `ARCHITECTURE.md` for the current system map and hard dependency rules.
3. Read `docs/QUALITY_SCORE.md` to see which domains or layers are weakest.
4. Read `docs/PLANS.md`, then open the active plan you are working from.
5. Read the relevant product spec in `docs/product-specs/`.
6. Run the standard bootstrap and verification path for this repo.
7. If baseline verification is failing, repair the baseline before adding scope.

## Routing Map

- `ARCHITECTURE.md`: domain map, layer model, dependency rules
- `docs/design-docs/index.md`: design decisions and core beliefs
- `docs/product-specs/index.md`: current product behaviors and acceptance targets
- `docs/PLANS.md`: plan lifecycle and execution-plan policy
- `docs/QUALITY_SCORE.md`: product-domain and layer health
- `docs/RELIABILITY.md`: runtime signals, benchmarks, and restart expectations
- `docs/SECURITY.md`: secrets, sandbox, data, and external-action rules
- `docs/FRONTEND.md`: UI constraints, design system rules, accessibility checks

## Working Contract

- Work from one bounded plan or feature slice at a time.
- Do not mark work done from code inspection alone; runnable evidence is required.
- If you change behavior, update the matching product, plan, or reliability docs in the same session.
- If you see repeated review feedback, promote it into a mechanical rule, check, or linter instead of re-explaining it in chat.
- Keep generated material in `docs/generated/` and source references in `docs/references/`.
- Prefer adding small, current docs over growing this file.

## Definition Of Done

A change is done only when all of the following are true:

- target behavior is implemented
- required verification actually ran
- evidence is linked from the relevant plan or quality document
- affected docs remain current
- the repository can restart cleanly from the standard startup path

## End Of Session

Before ending a session:

1. Update the active execution plan.
2. Update `docs/QUALITY_SCORE.md` if any domain or layer meaningfully changed.
3. Record new debt in `docs/exec-plans/tech-debt-tracker.md` if you deferred it.
4. Move finished plans to `docs/exec-plans/completed/` when appropriate.
5. Leave the repo in a restartable state with a clear next action.
"#
    .to_string()
}

fn architecture_md() -> String {
    r#"# Architecture

This document is the top-level system map. Keep it current enough that a fresh
agent can route changes without rediscovering the whole repository.

## System Overview

- Product or service: `[describe the system in one paragraph]`
- Primary users: `[who uses it]`
- Main runtime surfaces: `[web app, API, worker, CLI, mobile app, etc.]`
- Data stores and external services: `[list durable dependencies]`

## Layer Model

Adapt this starter model to the repository:

```text
Types -> Config -> Repository -> Service -> Runtime -> UI
                 Providers -> Service
```

## Boundary Rules

- Parse external data at the boundary before business logic sees it.
- Keep provider-specific code behind adapters or provider modules.
- Keep UI code from importing repository or storage internals directly.
- Keep generated clients and schemas as the source of truth for wire shapes.
- Document every intentional boundary exception with owner, reason, and removal trigger.

## Verification

- Architecture check: `[command or manual check]`
- Type/static check: `[command]`
- Runtime or integration check: `[command]`

## Change Rule

Update this file whenever a new runtime, layer, provider, generated artifact, or
cross-cutting concern changes where code belongs.
"#
    .to_string()
}

fn work_notes_plans_md() -> String {
    r#"# PLANS.md

This file defines how execution plans, research notes, and session logs are
created, updated, completed, and promoted.

## Default Location

Scratch specs, implementation plans, research notes, and execution logs belong
outside the repository by default:

```text
$CODEX_HOME/work-notes/<repo-slug>/<branch-or-thread>/
```

Tracked planning docs are exceptions. Use tracked docs only when the user asks
for repo-maintained plans, an issue or pull request explicitly governs the work,
or audit/history requirements make the artifact durable.

## When A Plan Is Required

Create a scratch plan when work:

- spans more than one session
- changes more than one subsystem
- has non-trivial verification or rollout risk
- depends on open decisions that should be logged

## Promotion Rules

Promote only durable knowledge into the repo:

- architecture invariants -> `ARCHITECTURE.md`
- product behavior -> `docs/product-specs/`
- design rationale -> `docs/design-docs/`
- operational procedure -> `docs/RELIABILITY.md` or a runbook
- durable security rule -> `docs/SECURITY.md`
- durable deferred work -> `docs/TECH_DEBT.md`

Do not promote raw logs, transcripts, secrets, customer data, production payloads,
or one-off debugging notes. Record sanitized evidence and links instead.

## Minimum Scratch Plan Sections

- objective
- scope and out-of-scope
- verification path
- risks and blockers
- progress log
- open decisions
- promotion target, if any

## Operating Rules

- One active plan should have one clearly owned current step.
- Update the scratch plan as work progresses; do not treat it as static prose.
- If a decision changes implementation direction, record it in the plan.
- Close each scratch note as done, superseded, or abandoned when work stops.
"#
    .to_string()
}

fn tracked_plans_md() -> String {
    r#"# PLANS.md

This file defines how execution plans are created, updated, completed, and
archived.

## When A Plan Is Required

Create an execution plan when work:

- spans more than one session
- changes more than one subsystem
- has non-trivial verification or rollout risk
- depends on open decisions that should be logged

## Plan Locations

- `docs/exec-plans/active/`: plans currently driving work
- `docs/exec-plans/completed/`: finished plans kept for future agent context
- `docs/exec-plans/tech-debt-tracker.md`: deferred work and follow-ups

## Minimum Plan Sections

- objective
- scope and out-of-scope
- verification path
- risks and blockers
- progress log
- open decisions

## Operating Rules

- One active plan should have one clearly owned current step.
- Update the plan as work progresses; do not treat it as static prose.
- If a decision changes implementation direction, record it in the plan.
- Move finished plans to `completed/` so agents can still discover prior context.
"#
    .to_string()
}

fn design_md() -> String {
    "# DESIGN.md\n\nUse this file for durable product and interaction design constraints.\n\n## Current Design System\n\n- Components: `[source of truth]`\n- Tokens: `[source of truth]`\n- Accessibility checks: `[command or manual check]`\n\n## Change Rule\n\nUpdate this file when UI primitives, layout rules, or interaction patterns change.\n"
        .to_string()
}

fn frontend_md() -> String {
    "# FRONTEND.md\n\nFrontend changes must preserve usability, accessibility, and runtime stability.\n\n## Rules\n\n- Prefer existing components and design tokens.\n- Validate responsive layouts before claiming completion.\n- Do not introduce decorative UI that hides operational workflows.\n- Record browser/runtime evidence for user-facing changes.\n\n## Verification\n\n- Static check: `[command]`\n- Browser check: `[command or Playwright path]`\n"
        .to_string()
}

fn product_sense_md() -> String {
    "# PRODUCT_SENSE.md\n\nThis file records product beliefs that should steer implementation choices.\n\n## Users\n\n- `[primary user]`: `[goal]`\n\n## Product Rules\n\n- `[rule]`\n\n## Change Rule\n\nUpdate this file when user-visible behavior, prioritization, or product language changes.\n"
        .to_string()
}

fn quality_score_md() -> String {
    "# QUALITY_SCORE.md\n\nUse this file to track quality by product domain and architecture layer.\n\n| Area | Score | Evidence | Next Trigger |\n| --- | --- | --- | --- |\n| `[area]` | `[A/B/C/D]` | `[test, audit, review, incident]` | `[when to revisit]` |\n\n## Scoring\n\n- A: reliable, tested, documented, and observable.\n- B: usable with minor known gaps.\n- C: works in common cases but lacks coverage or clarity.\n- D: fragile, unclear, or repeatedly blocks agents.\n\nUpdate this file when a task materially changes quality, reliability, or maintainability.\n"
        .to_string()
}

fn reliability_md() -> String {
    "# RELIABILITY.md\n\nThis file records runtime expectations and failure modes.\n\n## Startup\n\n- Command: `[command]`\n- Expected healthy signal: `[log, endpoint, UI state, metric]`\n\n## Critical Flows\n\n| Flow | Verification | Failure Signal | Recovery |\n| --- | --- | --- | --- |\n| `[flow]` | `[command]` | `[signal]` | `[step]` |\n\n## Change Rule\n\nUpdate this file when startup, deploy, background jobs, retries, or observability change.\n"
        .to_string()
}

fn security_md(plan_mode: PlanMode) -> String {
    let plan_reference = match plan_mode {
        PlanMode::WorkNotes => "the active scratch note or durable decision doc",
        PlanMode::Tracked => "the active plan",
    };
    format!(
        r#"# SECURITY.md

This file records security and privacy rules agents must follow.

## Rules

- Never commit secrets, tokens, cookies, private keys, or full production payloads.
- Report secret presence with booleans, lengths, or match status instead of raw values.
- Treat customer data and private operational data as sensitive by default.
- New dependencies need justification in {plan_reference}.
- Any command that mutates production, deletes data, changes permissions, or sends external messages needs explicit user approval.

## Verification

- Secret scan: `[command]`
- Dependency review: `[command]`
- Authz/security test: `[command]`

## Change Rule

Update this file when auth, permissions, secret handling, third-party data, or production operations change.
"#
    )
}

fn design_docs_index_md() -> String {
    "# Design Docs Index\n\nUse this index as the discoverable map of design history.\n\n## Accepted\n\n- `core-beliefs.md`: agent-first operating beliefs and durable project norms\n\n## Proposed\n\n- `[add new design doc paths here]`\n\n## Deprecated\n\n- `[move old or superseded design docs here with replacement links]`\n\n## Maintenance Rules\n\n- Every design doc should have an owner or update trigger.\n- Remove stale docs or mark them deprecated instead of letting them drift.\n- Link active scratch notes or tracked plans to the design docs they depend on.\n"
        .to_string()
}

fn core_beliefs_md() -> String {
    "# Core Beliefs\n\n- Repository-local docs are the system of record.\n- `AGENTS.md` is a router, not an encyclopedia.\n- Runnable evidence beats confidence.\n- Repeated review feedback should become a check, test, type, or rule.\n"
        .to_string()
}

fn generated_db_schema_md() -> String {
    "# Generated Database Schema\n\nThis file is a placeholder for generated schema output.\n\n## Source Command\n\n```bash\n[command that regenerates this file]\n```\n\nDo not hand-edit generated sections. Update the source schema and regenerate.\n"
        .to_string()
}

fn product_specs_index_md() -> String {
    "# Product Specs Index\n\nUse this index to find current user-visible behavior.\n\n## Current Specs\n\n- `new-user-onboarding.md`: starter example; replace or delete after adding real specs\n\n## Maintenance Rules\n\n- Specs should state current behavior before background.\n- Specs should link verification evidence or acceptance checks.\n"
        .to_string()
}

fn new_user_onboarding_md() -> String {
    "# New User Onboarding\n\nThis is a placeholder product spec.\n\n## Current Behavior\n\n`[describe the current behavior]`\n\n## Acceptance Checks\n\n- `[check]`\n\nReplace this file with a real spec or delete it once the first project-specific spec exists.\n"
        .to_string()
}

fn tech_debt_tracker_md() -> String {
    "# Tech Debt Tracker\n\nUse this file for debt that is real, acknowledged, and intentionally deferred.\n\n| Date | Area | Debt | Why Deferred | Risk | Next Trigger |\n|------|------|------|--------------|------|--------------|\n| YYYY-MM-DD | `[area]` | `[debt]` | `[reason]` | `[risk]` | `[when to revisit]` |\n"
        .to_string()
}

fn active_plans_index_md() -> String {
    "# Active Plans\n\nKeep one markdown file per active execution plan in this folder.\n\nSuggested filename pattern:\n\n- `YYYY-MM-DD-short-topic.md`\n\nEach active plan should be current enough that a fresh agent session can resume work from the repository alone.\n"
        .to_string()
}

fn completed_plans_index_md() -> String {
    "# Completed Plans\n\nMove finished plans here instead of deleting them. Completed plans are part of the repository memory surface and help later agent runs understand why the code looks the way it does.\n"
        .to_string()
}

fn design_system_ref() -> String {
    "Design system reference placeholder for LLM-readable tokens, components, and usage rules.\nReplace this with a generated or curated local reference.\n".to_string()
}

fn nixpacks_ref() -> String {
    "Nixpacks reference placeholder. Record the version, source URL, and deployment rules this repo relies on.\n".to_string()
}

fn uv_ref() -> String {
    "uv reference placeholder. Record the version, source URL, and package-management commands this repo relies on.\n".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashSet;
    use tempfile::tempdir;

    fn template_paths(plan_mode: PlanMode) -> Vec<&'static str> {
        template_files(plan_mode)
            .into_iter()
            .map(|file| file.path)
            .collect()
    }

    #[test]
    fn work_notes_mode_keeps_execution_plans_out_of_repo() {
        let paths = template_paths(PlanMode::WorkNotes);

        assert!(paths.contains(&"docs/TECH_DEBT.md"));
        assert!(
            paths
                .iter()
                .all(|path| !path.starts_with("docs/exec-plans/")),
            "work-notes mode must not create tracked execution-plan files: {paths:?}"
        );
    }

    #[test]
    fn tracked_mode_includes_execution_plan_index_files() {
        let paths = template_paths(PlanMode::Tracked);

        for expected_path in [
            "docs/exec-plans/active/index.md",
            "docs/exec-plans/completed/index.md",
            "docs/exec-plans/tech-debt-tracker.md",
        ] {
            assert!(
                paths.contains(&expected_path),
                "tracked mode is missing {expected_path}"
            );
        }
        assert!(!paths.contains(&"docs/TECH_DEBT.md"));
    }

    #[test]
    fn bundled_templates_and_sops_are_retrievable() {
        for file in template_files(PlanMode::WorkNotes) {
            assert_eq!(get_template_or_sop(file.path), Some(file.content));
        }

        for (name, content, _) in SOP_FILES {
            assert_eq!(get_template_or_sop(name), Some((*content).to_string()));
            assert_eq!(
                get_template_or_sop(&format!("{name}.md")),
                Some((*content).to_string())
            );
        }
    }

    #[test]
    fn dry_run_apply_does_not_write_files() {
        let target = tempdir().expect("create temporary target");
        let args = RepoApplyArgs {
            target: target.path().to_path_buf(),
            pack: PACK_NAME.to_string(),
            plan_mode: PlanMode::WorkNotes,
            dry_run: true,
            mode: ApplyMode::FillMissing,
            force: false,
        };

        let result = apply_files(&args).expect("dry-run apply succeeds");

        assert!(!result.created.is_empty());
        for path in result.created {
            assert!(
                !target.path().join(&path).exists(),
                "dry-run created unexpected file {path}"
            );
        }
    }

    #[test]
    fn apply_is_idempotent_for_matching_files() {
        let target = tempdir().expect("create temporary target");
        let args = RepoApplyArgs {
            target: target.path().to_path_buf(),
            pack: PACK_NAME.to_string(),
            plan_mode: PlanMode::WorkNotes,
            dry_run: false,
            mode: ApplyMode::FillMissing,
            force: false,
        };

        let first = apply_files(&args).expect("first apply succeeds");
        let second = apply_files(&args).expect("second apply succeeds");

        assert!(!first.created.is_empty());
        assert_eq!(
            second.skipped.len(),
            template_files(PlanMode::WorkNotes).len()
        );
        assert!(second.created.is_empty());
        assert!(second.conflicts.is_empty());
    }

    #[test]
    fn apply_reports_conflict_without_overwriting_existing_file() {
        let target = tempdir().expect("create temporary target");
        let agents = target.path().join("AGENTS.md");
        fs::write(&agents, "custom repo instructions\n").expect("write existing file");

        let args = RepoApplyArgs {
            target: target.path().to_path_buf(),
            pack: PACK_NAME.to_string(),
            plan_mode: PlanMode::WorkNotes,
            dry_run: false,
            mode: ApplyMode::FillMissing,
            force: false,
        };

        let result = apply_files(&args).expect("apply succeeds with conflict");

        assert!(result.conflicts.contains(&"AGENTS.md".to_string()));
        assert_eq!(
            fs::read_to_string(agents).expect("read existing file"),
            "custom repo instructions\n"
        );
    }

    #[test]
    fn external_command_exit_code_is_reported_as_data() {
        let result = run_external(
            "test",
            vec![
                PathBuf::from("sh"),
                PathBuf::from("-c"),
                PathBuf::from("printf out; printf err >&2; exit 7"),
            ],
            None,
        )
        .expect("external command is captured");

        assert_eq!(result["exit_code"], 7);
        assert_eq!(result["stdout"], "out");
        assert_eq!(result["stderr"], "err");
    }

    #[test]
    fn template_check_fails_when_applied_docs_are_still_exact_templates() {
        let target = tempdir().expect("create temporary target");
        let args = RepoApplyArgs {
            target: target.path().to_path_buf(),
            pack: PACK_NAME.to_string(),
            plan_mode: PlanMode::WorkNotes,
            dry_run: false,
            mode: ApplyMode::FillMissing,
            force: false,
        };
        apply_files(&args).expect("apply templates");

        let check = check_for_stale_templates(target.path()).expect("check templates");

        assert!(!check.ok);
        assert!(check.stale_templates.contains(&"AGENTS.md".to_string()));
        assert!(check.stale_templates.contains(&"docs/PLANS.md".to_string()));
        assert_eq!(
            check.stale_templates.len(),
            check.stale_templates.iter().collect::<BTreeSet<_>>().len()
        );
        assert!(check.findings.iter().any(|finding| {
            finding.path == "AGENTS.md" && finding.reason.contains("starter template")
        }));
        assert!(check.message.contains("starter templates"));
    }

    #[test]
    fn inspect_reports_stale_template_status() {
        let target = tempdir().expect("create temporary target");
        let args = RepoApplyArgs {
            target: target.path().to_path_buf(),
            pack: PACK_NAME.to_string(),
            plan_mode: PlanMode::WorkNotes,
            dry_run: false,
            mode: ApplyMode::FillMissing,
            force: false,
        };
        apply_files(&args).expect("apply templates");

        let inspection = inspect_repo(target.path()).expect("inspect repo");

        assert!(!inspection.template_check.ok);
        assert!(
            inspection
                .template_check
                .stale_templates
                .contains(&"AGENTS.md".to_string())
        );
    }

    #[test]
    fn template_check_passes_when_existing_harness_docs_are_customized() {
        let target = tempdir().expect("create temporary target");
        fs::write(
            target.path().join("AGENTS.md"),
            "custom agent instructions\n",
        )
        .expect("write custom agents");
        fs::create_dir_all(target.path().join("docs")).expect("create docs");
        fs::write(
            target.path().join("docs/PLANS.md"),
            "custom planning policy\n",
        )
        .expect("write custom plans");

        let check = check_for_stale_templates(target.path()).expect("check templates");

        assert!(check.ok);
        assert!(check.stale_templates.is_empty());
        assert!(check.findings.is_empty());
    }

    #[test]
    fn template_check_fails_when_docs_keep_unresolved_placeholders() {
        let target = tempdir().expect("create temporary target");
        fs::create_dir_all(target.path().join("docs")).expect("create docs");
        fs::write(
            target.path().join("docs/QUALITY_SCORE.md"),
            "# Quality\n\n| Area | Score | Evidence | Next Trigger |\n| --- | --- | --- | --- |\n| `[area]` | A | custom evidence | custom trigger |\n",
        )
        .expect("write placeholder doc");

        let check = check_for_stale_templates(target.path()).expect("check templates");

        assert!(!check.ok);
        assert!(
            check
                .findings
                .iter()
                .any(|finding| finding.path == "docs/QUALITY_SCORE.md"
                    && finding.reason.contains("placeholder"))
        );
    }

    #[test]
    fn external_command_exit_code_is_forced_to_failure_when_templates_are_stale() {
        let target = tempdir().expect("create temporary target");
        let args = RepoApplyArgs {
            target: target.path().to_path_buf(),
            pack: PACK_NAME.to_string(),
            plan_mode: PlanMode::WorkNotes,
            dry_run: false,
            mode: ApplyMode::FillMissing,
            force: false,
        };
        apply_files(&args).expect("apply templates");

        let result = run_external(
            "test",
            vec![
                PathBuf::from("sh"),
                PathBuf::from("-c"),
                PathBuf::from("exit 0"),
            ],
            Some(target.path()),
        )
        .expect("external command is captured");

        assert_eq!(result["exit_code"], 1);
        assert_eq!(result["template_check"]["ok"], false);
        assert!(
            result["template_check"]["message"]
                .as_str()
                .expect("template check message")
                .contains("starter templates")
        );
        assert!(
            result["template_check"]["stale_templates"]
                .as_array()
                .expect("stale template list")
                .iter()
                .any(|path| path == "AGENTS.md")
        );
    }

    #[test]
    fn audit_outcome_fails_unless_external_exit_code_is_zero() {
        let cli = Cli {
            json: true,
            command: Commands::Audit(TargetArgs {
                target: PathBuf::from("."),
            }),
        };
        let success = json!({ "exit_code": 0 });
        let failure = json!({ "exit_code": 1 });
        let missing = json!({ "stdout": "terminated before exit code" });

        assert!(!audit_command_should_fail(&cli.command, &success));
        assert!(audit_command_should_fail(&cli.command, &failure));
        assert!(audit_command_should_fail(&cli.command, &missing));
    }

    proptest! {
        #[test]
        fn template_paths_are_relative_unique_and_safe(plan_mode in prop_oneof![Just(PlanMode::WorkNotes), Just(PlanMode::Tracked)]) {
            let paths = template_paths(plan_mode);
            let unique_paths: HashSet<_> = paths.iter().copied().collect();

            prop_assert_eq!(paths.len(), unique_paths.len());
            for path in paths {
                let path_buf = Path::new(path);
                prop_assert!(!path.is_empty());
                prop_assert!(!path_buf.is_absolute(), "template path must be relative: {path}");
                prop_assert!(
                    !path_buf.components().any(|component| matches!(component, std::path::Component::ParentDir)),
                    "template path must not traverse upward: {path}"
                );
            }
        }

        #[test]
        fn only_the_supported_pack_is_accepted(pack in "\\PC{0,64}") {
            let result = ensure_pack(&pack);

            if pack == PACK_NAME {
                prop_assert!(result.is_ok());
            } else {
                prop_assert!(result.is_err());
            }
        }

        #[test]
        fn request_paths_containing_parent_traversal_are_rejected(prefix in "[/a-zA-Z0-9._-]{0,16}", suffix in "[/a-zA-Z0-9._-]{0,16}") {
            let path = format!("{prefix}..{suffix}");

            prop_assert!(sanitize_request_path(&path).is_err());
        }

        #[test]
        fn request_paths_without_parent_traversal_trim_leading_slashes(path in "[/a-zA-Z0-9._-]{0,64}") {
            prop_assume!(!path.contains(".."));

            let sanitized = sanitize_request_path(&path).expect("safe request path");

            prop_assert_eq!(sanitized.as_str(), path.trim_start_matches('/'));
            prop_assert!(!sanitized.contains(".."));
        }

        #[test]
        fn plan_reports_create_skip_or_conflict_for_existing_state(existing_content in "\\PC{0,256}") {
            let target = tempdir().expect("create temporary target");
            let template = template_files(PlanMode::WorkNotes)
                .into_iter()
                .find(|file| file.path == "AGENTS.md")
                .expect("AGENTS.md template exists");
            fs::write(target.path().join(template.path), &existing_content)
                .expect("write existing template path");

            let plans = plan_files(target.path(), PlanMode::WorkNotes)
                .expect("plan files succeeds");
            let agents_plan = plans
                .iter()
                .find(|plan| plan.path == template.path)
                .expect("AGENTS.md plan exists");

            if existing_content == template.content {
                prop_assert_eq!(agents_plan.action.as_str(), "skip");
                prop_assert_eq!(agents_plan.reason.as_str(), "file already matches template");
            } else {
                prop_assert_eq!(agents_plan.action.as_str(), "conflict");
                prop_assert_eq!(agents_plan.reason.as_str(), "file exists with different content");
            }

            for plan in plans {
                prop_assert!(
                    matches!(plan.action.as_str(), "create" | "skip" | "conflict"),
                    "unexpected plan action {} for {}",
                    plan.action,
                    plan.path
                );
            }
        }
    }
}
