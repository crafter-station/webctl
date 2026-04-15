use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow};
use clap::{Parser, Subcommand, ValueEnum};
use tokio::io::{AsyncBufReadExt, BufReader};
use url::Url;
use webctl_classifier::ax_stub::extract_ax_actions;
use webctl_classifier::http_infer::infer_endpoints;

use crate::commands::{emit, install, lint, recon};
use crate::ui::prompt;

#[derive(Debug, Parser)]
#[command(name = "webctl", about = "CLI-ify the web")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Recon(ReconArgs),
    Emit(EmitArgs),
    Install(InstallArgs),
    Lint(LintArgs),
    Exec(ExecArgs),
}

#[derive(Debug, Clone, clap::Args)]
#[command(disable_help_flag = true)]
pub struct ExecArgs {
    pub site: String,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, clap::Args)]
pub struct ReconArgs {
    pub url: String,

    #[arg(long, value_enum, default_value = "read-only")]
    pub policy: ProbePolicy,

    #[arg(long)]
    pub yes: bool,

    #[arg(long)]
    pub auto: bool,

    #[arg(long, group = "technique")]
    pub http: bool,

    #[arg(long, group = "technique")]
    pub ax: bool,

    #[arg(long, group = "technique")]
    pub hybrid: bool,

    #[arg(long)]
    pub output: Option<PathBuf>,
}

impl ReconArgs {
    pub fn technique_override(&self) -> Option<TechniqueOverride> {
        if self.http {
            Some(TechniqueOverride::Http)
        } else if self.ax {
            Some(TechniqueOverride::Ax)
        } else if self.hybrid {
            Some(TechniqueOverride::Hybrid)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
pub enum ProbePolicy {
    ReadOnly,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TechniqueOverride {
    Http,
    Ax,
    Hybrid,
}

#[derive(Debug, Clone, clap::Args)]
pub struct InstallArgs {
    pub source: String,

    #[arg(long = "dest")]
    pub dest: Option<PathBuf>,
}

#[derive(Debug, Clone, clap::Args)]
pub struct EmitArgs {
    #[command(subcommand)]
    pub target: EmitTargetArg,

    #[arg(long = "out-dir")]
    pub out_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum EmitTargetArg {
    Cli { ir_path: PathBuf },
}

#[derive(Debug, Clone, clap::Args)]
pub struct LintArgs {
    pub ir_path: PathBuf,
}

#[derive(Debug)]
pub struct ClassifierSuggestionView {
    pub bucket: webctl_classifier::ClassifierBucket,
    pub confidence: webctl_classifier::Confidence,
    pub http_endpoint_count: usize,
    pub ax_action_count: usize,
}

#[derive(Debug)]
pub struct InstallProgressView {
    pub source_label: String,
    pub version_label: String,
}

#[derive(Debug)]
pub struct InstallSuccessView {
    pub site_name: String,
    pub command_count: usize,
    pub hint: String,
}

pub async fn run_cli(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Recon(args) => recon::run(args).await,
        Commands::Emit(args) => emit::run(args).await,
        Commands::Install(args) => install::run(args).await,
        Commands::Lint(args) => lint::run(args),
        Commands::Exec(args) => exec_command(args).await,
    }
}

pub async fn exec_command(args: ExecArgs) -> anyhow::Result<()> {
    let home = home_dir()?;
    let ir_path = webctl_ir::site_ir_path(&home, &args.site);

    if !ir_path.exists() {
        let registry_path = webctl_ir::registry_path(&home);
        let registry = webctl_ir::RegistryIndex::load(&registry_path).unwrap_or_else(|_| webctl_ir::RegistryIndex { sites: Vec::new() });

        if let Some(entry) = registry.find(&args.site) {
            let descriptor = webctl_ir::read_ir(&entry.ir_path)
                .with_context(|| format!("failed to read IR for site '{}'", args.site))?;
            return exec_with_ir(&descriptor, &args).await;
        }

        let installed = registry.sites.iter().map(|s| &s.site_name).collect::<Vec<_>>();
        if installed.is_empty() {
            return Err(anyhow!("site '{}' not found. No sites installed.\n\n  Install one with:\n    webctl recon <url> --auto --yes\n    webctl install <ir_path>", args.site));
        }
        return Err(anyhow!("site '{}' not found. Installed sites:\n{}\n\n  Install with:\n    webctl install <ir_path>",
            args.site,
            installed.iter().map(|s| format!("    {s}")).collect::<Vec<_>>().join("\n")
        ));
    }

    let descriptor = webctl_ir::read_ir(&ir_path)
        .with_context(|| format!("failed to read IR for site '{}'", args.site))?;
    exec_with_ir(&descriptor, &args).await
}

async fn exec_with_ir(descriptor: &webctl_ir::SiteDescriptor, args: &ExecArgs) -> anyhow::Result<()> {
    let is_help = args.args.is_empty()
        || args.args.iter().any(|a| a == "--help" || a == "-h");

    if is_help {
        let help = webctl_emit_cli::build_help_text(descriptor);
        println!("{help}");
        return Ok(());
    }

    let is_json = args.args.iter().any(|a| a == "--json");
    let cmd_args: Vec<&str> = args.args.iter()
        .map(|s| s.as_str())
        .filter(|s| *s != "--json")
        .collect();

    let command_key = cmd_args.join(" ");
    let operation = descriptor.operations.iter().find(|op| {
        op.command_path.join(" ") == command_key
    });

    let Some(operation) = operation else {
        let available = webctl_ir::command_help_rows(descriptor)
            .iter()
            .map(|r| format!("    {}  {}", r.command, r.description))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(anyhow!(
            "unknown command '{}' for site '{}'\n\nAvailable commands:\n{}\n",
            command_key, args.site, available
        ));
    };

    let url = match &operation.transport {
        webctl_ir::OperationTransport::Http(http_op) => {
            let endpoint = descriptor.http.as_ref()
                .and_then(|h| h.endpoints.get(http_op.endpoint_index))
                .ok_or_else(|| anyhow!("endpoint index {} out of bounds", http_op.endpoint_index))?;
            let base = url::Url::parse(&descriptor.meta.source_url)
                .map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or("localhost")))
                .unwrap_or_else(|_| descriptor.meta.source_url.clone());
            format!("{}{}", base, endpoint.path)
        }
        webctl_ir::OperationTransport::Ax(_) => {
            return Err(anyhow!("AX-based command execution is not yet implemented"));
        }
    };

    let result = crate::execute::fetch_page(&url).await
        .with_context(|| format!("failed to fetch {url}"))?;

    if is_json {
        let json = crate::execute::format_json(&result)?;
        println!("{json}");
    } else {
        let formatted = crate::execute::format_human(&result, &args.site, &command_key);
        println!("{formatted}");
    }

    Ok(())
}

pub async fn recon_command(args: ReconArgs) -> anyhow::Result<webctl_ir::SiteDescriptor> {
    let output_dir = recon_output_dir(&args.url, args.output.as_deref())?;
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create output dir {}", output_dir.display()))?;

    let probe_options = webctl_probe::ProbeOptions {
        url: args.url.clone(),
        visible: true,
        output_dir: output_dir.clone(),
    };

    eprintln!("⠋ Connecting to browser on port 9222...");
    let browser = webctl_probe::agent_browser::BrowserProcess {
        child_id: 0,
        cdp_port: 9222,
        profile_dir: std::path::PathBuf::new(),
    };
    let session = match webctl_probe::agent_browser::connect_session(browser, &probe_options).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("✗ Cannot connect to browser on port 9222\n");
            eprintln!("  webctl needs a Chromium browser with remote debugging enabled.");
            eprintln!("  Start one with:\n");
            eprintln!("    # Comet (Perplexity)");
            eprintln!("    /Applications/Comet.app/Contents/MacOS/Comet --remote-debugging-port=9222 &\n");
            eprintln!("    # Chrome");
            eprintln!("    /Applications/Google\\ Chrome.app/Contents/MacOS/Google\\ Chrome --remote-debugging-port=9222 &\n");
            eprintln!("  Then retry:");
            eprintln!("    webctl recon {} {}", args.url, if args.auto { "--auto" } else { "" });
            return Err(e);
        }
    };
    eprintln!("✓ Connected");

    eprintln!("⠋ Starting HAR capture...");
    webctl_probe::agent_browser::start_har_capture(&session).await?;

    let initial_title = webctl_probe::agent_browser::get_title(&session).await.ok();
    let initial_url = webctl_probe::agent_browser::get_url(&session).await.ok();
    eprintln!("✓ Navigated to: {}", initial_title.as_deref().unwrap_or(&args.url));

    let ax_pre_path = webctl_probe::paths::ax_pre_path(&output_dir);
    webctl_probe::agent_browser::take_ax_snapshot(&session, &ax_pre_path).await?;

    if args.auto {
        eprintln!("⠋ Auto-exploring {}...", args.url);
        let auto_result = webctl_probe::run_auto_recon(&session, |iter, elements, url| {
            eprint!("\r  iter {iter}/15  elements: {elements:<4}  current: {url}          ");
        })
        .await
        .context("auto-recon failed")?;
        eprintln!();
        eprintln!(
            "✓ Explored {} pages in {} iterations — {}",
            auto_result.pages_visited, auto_result.iterations, auto_result.stop_reason
        );
    } else {
        eprintln!("Navigate the site in the browser window, then press ENTER here when done.");
        wait_for_enter().await?;
    }

    eprintln!("⠋ Finalizing capture...");
    let mut probe = webctl_probe::finalize_capture(session).await?;

    if let Some(ref title) = initial_title {
        if !title.is_empty() {
            probe.final_title = Some(title.clone());
        }
    }
    if let Some(ref url) = initial_url {
        if !url.is_empty() && probe.final_url.is_none() {
            probe.final_url = Some(url.clone());
        }
    }

    eprintln!("✓ Captured {} HTTP requests", probe.har_entry_count);
    eprintln!("⠋ Classifying site...");
    let har_bytes = webctl_probe::read_har_bytes(&probe.har_path)?;
    let har = webctl_probe::har::parse_har(&har_bytes)?;
    let ax_tree = read_optional_string(probe.ax_final_path.as_deref())?;
    let suggestion = webctl_classifier::classify(&probe, &har_bytes, ax_tree.as_deref())?;

    let http_surface = webctl_ir::HttpSurface {
        endpoints: infer_endpoints(&har),
    };
    let ax_surface = ax_tree
        .as_deref()
        .map(|text| webctl_ir::AxSurface {
            actions: extract_ax_actions(text),
        });

    let view = ClassifierSuggestionView {
        bucket: suggestion.bucket.clone(),
        confidence: suggestion.confidence.clone(),
        http_endpoint_count: http_surface.endpoints.len(),
        ax_action_count: ax_surface.as_ref().map(|surface| surface.actions.len()).unwrap_or(0),
    };
    eprintln!("✓ Classified: {} ({} confidence)",
        classifier_bucket_label(&view.bucket),
        confidence_label(&view.confidence),
    );
    eprintln!("  HTTP endpoints: {}  |  AX actions: {}", view.http_endpoint_count, view.ax_action_count);

    let decision = apply_user_override(suggestion, args.technique_override());

    if !args.yes {
        let technique = technique_label(&decision);
        let accepted = prompt::confirm(&format!("Build IR using {technique}?"))?;
        if !accepted {
            return Err(anyhow!(abort_recon().to_string()));
        }
    }

    eprintln!("⠋ Building IR...");
    let descriptor = build_ir(decision, probe, http_surface, ax_surface)?;
    let ir_path = output_dir.join(format!("{}.webctl.json", descriptor.meta.site_name));
    webctl_ir::write_ir(&ir_path, &descriptor)
        .with_context(|| format!("failed to write IR to {}", ir_path.display()))?;

    let op_count = descriptor.operations.len();
    let ir_size = std::fs::metadata(&ir_path).map(|m| m.len()).unwrap_or(0);
    eprintln!("✓ IR written: {} ({} operations, {}KB)", ir_path.display(), op_count, ir_size / 1024);
    eprintln!();
    eprintln!("Next steps:");
    eprintln!("  webctl emit cli {}    Generate a CLI shim", ir_path.display());
    eprintln!("  webctl install {}     Install locally", ir_path.display());
    eprintln!("  webctl lint {}        Validate the IR", ir_path.display());

    Ok(descriptor)
}

pub fn apply_user_override(
    suggestion: webctl_classifier::ClassificationResult,
    override_choice: Option<TechniqueOverride>,
) -> webctl_classifier::TechniqueDecision {
    match override_choice {
        Some(TechniqueOverride::Http) => webctl_classifier::TechniqueDecision::HttpOnly,
        Some(TechniqueOverride::Ax) => webctl_classifier::TechniqueDecision::AxOnly,
        Some(TechniqueOverride::Hybrid) => webctl_classifier::TechniqueDecision::Hybrid,
        None => match suggestion.bucket {
            webctl_classifier::ClassifierBucket::FormSessionLegacy
            | webctl_classifier::ClassifierBucket::RestModernSpa
            | webctl_classifier::ClassifierBucket::GraphqlIntrospectable
            | webctl_classifier::ClassifierBucket::HtmlRendered => {
                webctl_classifier::TechniqueDecision::HttpOnly
            }
            webctl_classifier::ClassifierBucket::AxOnly => {
                webctl_classifier::TechniqueDecision::AxOnly
            }
            webctl_classifier::ClassifierBucket::Hostile
            | webctl_classifier::ClassifierBucket::Inconclusive => {
                if suggestion.features.ax_interactive_nodes > 0
                    && suggestion.features.total_requests > 0
                {
                    webctl_classifier::TechniqueDecision::Hybrid
                } else if suggestion.features.ax_interactive_nodes > 0 {
                    webctl_classifier::TechniqueDecision::AxOnly
                } else {
                    webctl_classifier::TechniqueDecision::HttpOnly
                }
            }
        },
    }
}

pub fn abort_recon() -> miette::Report {
    miette::miette!("recon aborted")
}

pub fn build_ir(
    decision: webctl_classifier::TechniqueDecision,
    probe: webctl_probe::ProbeCapture,
    http: webctl_ir::HttpSurface,
    ax: Option<webctl_ir::AxSurface>,
) -> anyhow::Result<webctl_ir::SiteDescriptor> {
    let source_url = probe
        .final_url
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let site_name = site_slug_from_url(&source_url).unwrap_or_else(|| "site".to_string());
    let display_name = probe
        .final_title
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| site_name.clone());
    let technique = match decision {
        webctl_classifier::TechniqueDecision::HttpOnly => webctl_ir::ProvenanceTechnique::Http,
        webctl_classifier::TechniqueDecision::AxOnly => webctl_ir::ProvenanceTechnique::Ax,
        webctl_classifier::TechniqueDecision::Hybrid => webctl_ir::ProvenanceTechnique::Hybrid,
        webctl_classifier::TechniqueDecision::Abort => return Err(anyhow!("recon aborted")),
    };

    let mut operations = Vec::new();
    let http_enabled = matches!(
        decision,
        webctl_classifier::TechniqueDecision::HttpOnly
            | webctl_classifier::TechniqueDecision::Hybrid
    );
    let ax_enabled = matches!(
        decision,
        webctl_classifier::TechniqueDecision::AxOnly | webctl_classifier::TechniqueDecision::Hybrid
    );

    if http_enabled {
        for (index, endpoint) in http.endpoints.iter().enumerate() {
            let command_path = if endpoint.namespace.is_empty() {
                fallback_http_command_path(index, &endpoint.path)
            } else {
                webctl_ir::normalize_command_path(&endpoint.namespace)
            };
            let summary = if endpoint.description.trim().is_empty() {
                format!("{:?} {}", endpoint.method, endpoint.path)
            } else {
                endpoint.description.clone()
            };
            let description = if endpoint.description.trim().is_empty() {
                summary.clone()
            } else {
                endpoint.description.clone()
            };
            operations.push(webctl_ir::OperationDescriptor {
                command_path,
                summary,
                description,
                operation_kind: endpoint.operation_kind.clone(),
                transport: webctl_ir::OperationTransport::Http(webctl_ir::HttpOperation {
                    endpoint_index: index,
                }),
            });
        }
    }

    if ax_enabled {
        if let Some(ref ax_surface) = ax {
            for (index, action) in ax_surface.actions.iter().enumerate() {
                let command_path = if action.command_path.is_empty() {
                    vec![format!("action-{}", index + 1)]
                } else {
                    action.command_path.clone()
                };
                let description = if action.description.trim().is_empty() {
                    format!("AX action {}", index + 1)
                } else {
                    action.description.clone()
                };
                operations.push(webctl_ir::OperationDescriptor {
                    command_path,
                    summary: description.clone(),
                    description,
                    operation_kind: webctl_ir::OperationKind::Other,
                    transport: webctl_ir::OperationTransport::Ax(webctl_ir::AxOperation {
                        action_index: index,
                    }),
                });
            }
        }
    }

    if operations.is_empty() {
        return Err(anyhow!("no operations were derived from the selected technique"));
    }

    Ok(webctl_ir::SiteDescriptor {
        meta: webctl_ir::SiteMeta {
            site_name,
            display_name,
            source_url,
            ir_version: "0.1.0".to_string(),
        },
        provenance: webctl_ir::Provenance {
            generated_at: timestamp_string()?,
            technique,
            classifier_bucket: technique_bucket_label(&decision).to_string(),
            probe_duration_sec: 0,
        },
        operations,
        http: http_enabled.then_some(http),
        ax: ax_enabled.then_some(ax).flatten(),
    })
}

pub async fn emit_command(args: EmitArgs) -> anyhow::Result<std::path::PathBuf> {
    let (ir_path, target_label) = match &args.target {
        EmitTargetArg::Cli { ir_path } => (ir_path.clone(), "cli"),
    };
    let descriptor = webctl_ir::read_ir(&ir_path)
        .with_context(|| format!("failed to read IR from {}", ir_path.display()))?;

    if let Err(errors) = webctl_ir::lint_ir(&descriptor) {
        for error in errors {
            println!("warning: {error}");
        }
    }

    let out_dir = args.out_dir.unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("emit")
            .join(target_label)
            .join(&descriptor.meta.site_name)
    });
    let emitted = webctl_emit_cli::emit_cli_shim(webctl_emit_cli::CliEmitRequest {
        descriptor,
        out_dir,
    })?;

    eprintln!("✓ Shim compiled: {} ({}KB)",
        emitted.binary_path.display(),
        emitted.binary_size / 1024
    );
    eprintln!();
    eprintln!("Next step:");
    eprintln!("  webctl install {}", ir_path.display());

    Ok(emitted.binary_path)
}

pub async fn install_command(args: InstallArgs) -> anyhow::Result<InstallSuccessView> {
    let source_path = resolve_local_ir_source(&args.source)?;
    let source_label = source_path.display().to_string();

    let descriptor = webctl_ir::read_ir(&source_path)
        .with_context(|| format!("failed to read IR from {}", source_path.display()))?;
    let progress = InstallProgressView {
        source_label,
        version_label: descriptor.meta.ir_version.clone(),
    };
    println!("{}", render_install_progress(&progress));

    if let Err(errors) = webctl_ir::lint_ir(&descriptor) {
        for error in errors {
            println!("warning: {error}");
        }
    }

    let home_dir = home_dir()?;
    let site_ir_path = webctl_ir::site_ir_path(&home_dir, &descriptor.meta.site_name);
    webctl_ir::write_ir(&site_ir_path, &descriptor)
        .with_context(|| format!("failed to copy IR to {}", site_ir_path.display()))?;

    let shim_build_dir = std::env::temp_dir().join(format!(
        "webctl-install-{}-{}",
        descriptor.meta.site_name,
        std::process::id()
    ));
    let emitted = webctl_emit_cli::emit_cli_shim(webctl_emit_cli::CliEmitRequest {
        descriptor: descriptor.clone(),
        out_dir: shim_build_dir,
    })?;

    let dest_dir = install_destination(&args)?;
    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("failed to create destination dir {}", dest_dir.display()))?;
    let installed_shim_path = dest_dir.join(&descriptor.meta.site_name);
    std::fs::copy(&emitted.binary_path, &installed_shim_path).with_context(|| {
        format!(
            "failed to copy shim from {} to {}",
            emitted.binary_path.display(),
            installed_shim_path.display()
        )
    })?;

    let registry_path = webctl_ir::registry_path(&home_dir);
    let mut registry = webctl_ir::RegistryIndex::load(&registry_path)
        .with_context(|| format!("failed to load registry at {}", registry_path.display()))?;
    registry.upsert(webctl_ir::InstalledSiteEntry {
        site_name: descriptor.meta.site_name.clone(),
        ir_path: site_ir_path.clone(),
        shim_path: installed_shim_path.clone(),
    });
    registry
        .save(&registry_path)
        .with_context(|| format!("failed to write registry at {}", registry_path.display()))?;

    let meta_path = webctl_ir::site_meta_path(&home_dir, &descriptor.meta.site_name);
    if let Some(parent) = meta_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create site metadata dir {}", parent.display()))?;
    }
    let install_record = webctl_ir::InstallRecord {
        site_name: descriptor.meta.site_name.clone(),
        ir_path: site_ir_path,
        shim_path: installed_shim_path,
        installed_at: timestamp_string()?,
        source: webctl_ir::InstallSource::LocalPath(webctl_ir::LocalPathSource {
            path: source_path,
        }),
    };
    std::fs::write(&meta_path, serde_json::to_vec_pretty(&install_record)?)
        .with_context(|| format!("failed to write install metadata to {}", meta_path.display()))?;

    let in_path = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .any(|p| std::path::Path::new(p) == dest_dir);

    let hint = if in_path {
        format!("Run: {} --help", descriptor.meta.site_name)
    } else {
        format!(
            "⚠ {} is not in your PATH. Add it:\n  echo 'export PATH=\"{}:$PATH\"' >> ~/.zshrc && source ~/.zshrc\n\nThen try:\n  {} --help",
            dest_dir.display(),
            dest_dir.display(),
            descriptor.meta.site_name
        )
    };

    let view = InstallSuccessView {
        site_name: descriptor.meta.site_name.clone(),
        command_count: descriptor.operations.len(),
        hint,
    };
    eprintln!("✓ Installed: {} ({} commands)", view.site_name, view.command_count);
    eprintln!("{}", view.hint);

    Ok(view)
}

pub fn render_classifier_suggestion(view: &ClassifierSuggestionView) -> String {
    format!(
        "Suggested bucket: {} ({})\nHTTP endpoints: {}\nAX actions: {}",
        classifier_bucket_label(&view.bucket),
        confidence_label(&view.confidence),
        view.http_endpoint_count,
        view.ax_action_count
    )
}

pub fn render_install_progress(view: &InstallProgressView) -> String {
    format!(
        "Installing IR from {} (version {})",
        view.source_label, view.version_label
    )
}

pub fn render_install_success(view: &InstallSuccessView) -> String {
    format!(
        "Installed {} with {} commands\n{}",
        view.site_name, view.command_count, view.hint
    )
}

fn fallback_http_command_path(index: usize, endpoint_path: &str) -> Vec<String> {
    let segments = endpoint_path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .take(2)
        .map(|segment| {
            segment
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || ch == '-' {
                        ch.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
                .trim_matches('-')
                .to_string()
        })
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        vec![format!("endpoint-{}", index + 1)]
    } else {
        segments
    }
}

fn technique_label(decision: &webctl_classifier::TechniqueDecision) -> &'static str {
    match decision {
        webctl_classifier::TechniqueDecision::HttpOnly => "http",
        webctl_classifier::TechniqueDecision::AxOnly => "ax",
        webctl_classifier::TechniqueDecision::Hybrid => "hybrid",
        webctl_classifier::TechniqueDecision::Abort => "abort",
    }
}

fn classifier_bucket_label(bucket: &webctl_classifier::ClassifierBucket) -> &'static str {
    match bucket {
        webctl_classifier::ClassifierBucket::FormSessionLegacy => "FormSessionLegacy",
        webctl_classifier::ClassifierBucket::RestModernSpa => "RestModernSpa",
        webctl_classifier::ClassifierBucket::GraphqlIntrospectable => "GraphqlIntrospectable",
        webctl_classifier::ClassifierBucket::AxOnly => "AxOnly",
        webctl_classifier::ClassifierBucket::HtmlRendered => "HtmlRendered",
        webctl_classifier::ClassifierBucket::Hostile => "Hostile",
        webctl_classifier::ClassifierBucket::Inconclusive => "Inconclusive",
    }
}

fn confidence_label(confidence: &webctl_classifier::Confidence) -> &'static str {
    match confidence {
        webctl_classifier::Confidence::High => "high",
        webctl_classifier::Confidence::Medium => "medium",
        webctl_classifier::Confidence::Low => "low",
    }
}

fn technique_bucket_label(decision: &webctl_classifier::TechniqueDecision) -> &'static str {
    match decision {
        webctl_classifier::TechniqueDecision::HttpOnly => "HttpOnly",
        webctl_classifier::TechniqueDecision::AxOnly => "AxOnly",
        webctl_classifier::TechniqueDecision::Hybrid => "Hybrid",
        webctl_classifier::TechniqueDecision::Abort => "Abort",
    }
}

fn home_dir() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

fn install_destination(args: &InstallArgs) -> anyhow::Result<PathBuf> {
    match &args.dest {
        Some(path) => Ok(path.clone()),
        None => Ok(home_dir()?.join(".local").join("bin")),
    }
}

fn resolve_local_ir_source(source: &str) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(source);
    if path.exists() {
        return Ok(path);
    }
    Err(anyhow!(
        "unsupported IR source `{source}`; only local file paths are supported in this build"
    ))
}

fn recon_output_dir(url: &str, output: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(output) = output {
        return Ok(output.to_path_buf());
    }

    let slug = site_slug_from_url(url).unwrap_or_else(|| sanitize_slug(url));
    Ok(std::env::current_dir()
        .context("failed to determine current directory")?
        .join(format!("webctl-recon-{slug}")))
}

fn site_slug_from_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    Some(sanitize_slug(host))
}

fn sanitize_slug(input: &str) -> String {
    let mut slug = String::with_capacity(input.len());
    let mut last_dash = false;

    for ch in input.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };

        if next == '-' {
            if !last_dash {
                slug.push('-');
            }
            last_dash = true;
        } else {
            slug.push(next);
            last_dash = false;
        }
    }

    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "site".to_string()
    } else {
        slug
    }
}

async fn wait_for_enter() -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .context("failed to read ENTER confirmation from stdin")?;
    Ok(())
}

fn read_optional_string(path: Option<&Path>) -> anyhow::Result<Option<String>> {
    path.map(|path| {
        std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))
    })
    .transpose()
}

fn timestamp_string() -> anyhow::Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_secs();
    Ok(format!("{now}"))
}

pub fn flush_stdout() -> anyhow::Result<()> {
    io::stdout().flush().context("failed to flush stdout")
}
