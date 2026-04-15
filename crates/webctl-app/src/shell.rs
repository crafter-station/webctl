use std::path::PathBuf;

use anyhow::Context;
use owo_colors::OwoColorize;
use rustyline::DefaultEditor;

use crate::cli::{ExecArgs, CheckArgs};

struct ShellState {
    current_site: Option<String>,
    current_descriptor: Option<webctl_ir::SiteDescriptor>,
    home: PathBuf,
}

pub async fn run_shell() -> anyhow::Result<()> {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .context("HOME not set")?;

    let mut state = ShellState {
        current_site: None,
        current_descriptor: None,
        home,
    };

    let mut rl = DefaultEditor::new().context("failed to init readline")?;

    print_welcome(&state);

    loop {
        let prompt = build_prompt(&state);
        let line = match rl.readline(&prompt) {
            Ok(line) => line,
            Err(rustyline::error::ReadlineError::Interrupted) => continue,
            Err(rustyline::error::ReadlineError::Eof) => break,
            Err(e) => return Err(e.into()),
        };

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let _ = rl.add_history_entry(line);

        let parts: Vec<&str> = line.split_whitespace().collect();
        let cmd = parts[0];
        let args = &parts[1..];

        match cmd {
            "exit" | "quit" | "q" => break,
            "help" | "?" => print_shell_help(&state),
            "ls" | "list" => list_sites(&state),
            "open" => {
                if args.is_empty() {
                    eprintln!("  usage: open <site>");
                } else if state.current_site.is_some() && args[0].parse::<usize>().is_ok() {
                    let full_args = if state.current_site.is_some() {
                        let site = state.current_site.as_ref().unwrap().clone();
                        let mut a = vec!["open"];
                        a.extend_from_slice(args);
                        dispatch_site_command(&site, &a, &state).await;
                    } else {
                        eprintln!("  no site selected. Run: open <site-name>");
                    };
                    let _ = full_args;
                } else {
                    open_site(args[0], &mut state);
                }
            }
            "switch" => {
                if args.is_empty() {
                    eprintln!("  usage: switch <site>");
                } else {
                    open_site(args[0], &mut state);
                }
            }
            "back" | "close" => {
                state.current_site = None;
                state.current_descriptor = None;
            }
            "check" => {
                if let Some(ref site) = state.current_site {
                    let _ = crate::check_command(CheckArgs { site: site.clone() }).await;
                } else if !args.is_empty() {
                    let _ = crate::check_command(CheckArgs { site: args[0].to_string() }).await;
                } else {
                    eprintln!("  usage: check <site>");
                }
            }
            _ => {
                if let Some(ref site) = state.current_site {
                    dispatch_site_command(site, &parts, &state).await;
                } else {
                    let registry_path = webctl_ir::registry_path(&state.home);
                    let registry = webctl_ir::RegistryIndex::load(&registry_path)
                        .unwrap_or_else(|_| webctl_ir::RegistryIndex { sites: Vec::new() });

                    if registry.find(cmd).is_some() {
                        open_site(cmd, &mut state);
                        if !args.is_empty() {
                            let site = state.current_site.as_ref().unwrap().clone();
                            dispatch_site_command(&site, args, &state).await;
                        }
                    } else {
                        eprintln!("  unknown command: {cmd}. Type 'help' for options.");
                    }
                }
            }
        }
    }

    eprintln!("goodbye");
    Ok(())
}

fn print_welcome(state: &ShellState) {
    let registry_path = webctl_ir::registry_path(&state.home);
    let registry = webctl_ir::RegistryIndex::load(&registry_path)
        .unwrap_or_else(|_| webctl_ir::RegistryIndex { sites: Vec::new() });

    if crate::execute::use_color() {
        eprintln!("{}", "webctl shell".bold());
        eprintln!("{}", format!("{} sites installed", registry.sites.len()).dimmed());
        eprintln!("{}", "Type 'help' for commands, 'ls' to list sites, 'exit' to quit.".dimmed());
    } else {
        eprintln!("webctl shell");
        eprintln!("{} sites installed", registry.sites.len());
        eprintln!("Type 'help' for commands, 'ls' to list sites, 'exit' to quit.");
    }
    eprintln!();
}

fn build_prompt(state: &ShellState) -> String {
    if crate::execute::use_color() {
        match &state.current_site {
            Some(site) => format!("{} {} ", site.cyan(), ">".dimmed()),
            None => format!("{} {} ", "webctl".bold(), ">".dimmed()),
        }
    } else {
        match &state.current_site {
            Some(site) => format!("{site} > "),
            None => "webctl > ".to_string(),
        }
    }
}

fn print_shell_help(state: &ShellState) {
    let color = crate::execute::use_color();
    eprintln!();
    if color {
        eprintln!("  {}", "Shell commands:".dimmed());
        eprintln!("    {}          {}", "ls".green(), "List installed sites".dimmed());
        eprintln!("    {}  {}", "open <site>".green(), "Enter a site context".dimmed());
        eprintln!("    {}    {}", "switch <s>".green(), "Switch to another site".dimmed());
        eprintln!("    {}        {}", "back".green(), "Leave current site context".dimmed());
        eprintln!("    {}   {}", "check [site]".green(), "Check for drift".dimmed());
        eprintln!("    {}        {}", "exit".green(), "Quit shell".dimmed());
    } else {
        eprintln!("  Shell commands:");
        eprintln!("    ls          List installed sites");
        eprintln!("    open <site> Enter a site context");
        eprintln!("    switch <s>  Switch to another site");
        eprintln!("    back        Leave current site context");
        eprintln!("    check       Check for drift");
        eprintln!("    exit        Quit shell");
    }

    if let Some(ref site) = state.current_site {
        if let Some(ref desc) = state.current_descriptor {
            eprintln!();
            if color {
                eprintln!("  {} ({})", format!("{site} commands:").dimmed(), desc.operations.len());
            } else {
                eprintln!("  {site} commands ({}):", desc.operations.len());
            }
            for row in webctl_ir::command_help_rows(desc).iter().take(8) {
                if color {
                    eprintln!("    {}  {}", row.command.green(), row.description.dimmed());
                } else {
                    eprintln!("    {}  {}", row.command, row.description);
                }
            }
            if desc.operations.len() > 8 {
                eprintln!("    ... and {} more", desc.operations.len() - 8);
            }
        }
    }
    eprintln!();
}

fn list_sites(state: &ShellState) {
    let registry_path = webctl_ir::registry_path(&state.home);
    let registry = webctl_ir::RegistryIndex::load(&registry_path)
        .unwrap_or_else(|_| webctl_ir::RegistryIndex { sites: Vec::new() });

    if registry.sites.is_empty() {
        eprintln!("  No sites installed. Run: webctl recon <url> --auto --yes");
        return;
    }

    let color = crate::execute::use_color();
    for entry in &registry.sites {
        let cmd_count = webctl_ir::read_ir(&entry.ir_path)
            .map(|d| d.operations.len())
            .unwrap_or(0);

        let active = state.current_site.as_deref() == Some(&entry.site_name);
        let marker = if active { "→ " } else { "  " };

        if color {
            let name = if active {
                format!("{}", entry.site_name.cyan().bold())
            } else {
                format!("{}", entry.site_name.white())
            };
            eprintln!("{marker}{name}  {}", format!("{cmd_count} commands").dimmed());
        } else {
            eprintln!("{marker}{}  {cmd_count} commands", entry.site_name);
        }
    }
}

fn open_site(name: &str, state: &mut ShellState) {
    let ir_path = webctl_ir::site_ir_path(&state.home, name);
    if !ir_path.exists() {
        eprintln!("  site '{name}' not found");
        return;
    }

    match webctl_ir::read_ir(&ir_path) {
        Ok(desc) => {
            let cmd_count = desc.operations.len();
            state.current_site = Some(name.to_string());
            state.current_descriptor = Some(desc);

            if crate::execute::use_color() {
                eprintln!("  {} ({} commands)", name.cyan().bold(), cmd_count);
            } else {
                eprintln!("  {name} ({cmd_count} commands)");
            }
        }
        Err(e) => {
            eprintln!("  failed to load site '{name}': {e}");
        }
    }
}

async fn dispatch_site_command(site: &str, parts: &[&str], _state: &ShellState) {
    let args_vec: Vec<String> = parts.iter().map(|s| s.to_string()).collect();

    let exec_args = ExecArgs {
        site: site.to_string(),
        args: args_vec,
    };

    if let Err(e) = crate::exec_command(exec_args).await {
        eprintln!("  error: {e:#}");
    }
}
