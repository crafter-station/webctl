use owo_colors::OwoColorize;

pub fn build_help_text(descriptor: &webctl_ir::SiteDescriptor) -> String {
    build_help_text_impl(descriptor, false)
}

pub fn build_help_text_colored(descriptor: &webctl_ir::SiteDescriptor) -> String {
    build_help_text_impl(descriptor, true)
}

fn build_help_text_impl(descriptor: &webctl_ir::SiteDescriptor, color: bool) -> String {
    let site_name = &descriptor.meta.site_name;
    let display_name = &descriptor.meta.display_name;
    let rows = webctl_ir::command_help_rows(descriptor);
    let command_width = rows.iter().map(|row| row.command.len()).max().unwrap_or(0);

    let mut out = String::new();

    if color {
        out.push_str(&format!("{}\n\n", format!("{site_name} — {display_name}").bold()));
    } else {
        out.push_str(&format!("{site_name} — {display_name}\n\n"));
    }

    if color {
        out.push_str(&format!("{}\n", "USAGE".dimmed()));
        out.push_str(&format!("  {} {} {}\n\n",
            site_name.cyan(),
            "<command>".white(),
            "[flags]".dimmed()
        ));
    } else {
        out.push_str("USAGE\n");
        out.push_str(&format!("  {site_name} <command> [flags]\n\n"));
    }

    if color {
        out.push_str(&format!("{}\n", "COMMANDS".dimmed()));
    } else {
        out.push_str("COMMANDS\n");
    }

    for row in &rows {
        if color {
            out.push_str(&format!("  {:width$}  {}\n",
                row.command.green(),
                row.description.dimmed(),
                width = command_width
            ));
        } else {
            out.push_str(&format!("  {:width$}  {}\n",
                row.command,
                row.description,
                width = command_width
            ));
        }
    }
    out.push('\n');

    if color {
        out.push_str(&format!("{}\n", "FLAGS".dimmed()));
        out.push_str(&format!("  {}    {}\n", "--json".green(), "Output as JSON".dimmed()));
        out.push_str(&format!("  {}    {}\n\n", "--help".green(), "Show this help".dimmed()));
    } else {
        out.push_str("FLAGS\n");
        out.push_str("  --json    Output as JSON\n");
        out.push_str("  --help    Show this help\n\n");
    }

    if color {
        out.push_str(&format!("{}\n", "LEARN MORE".dimmed()));
        out.push_str(&format!("  {}    {}\n",
            format!("webctl check {site_name}").cyan(),
            "Check for drift".dimmed()
        ));
        out.push_str(&format!("  {}   {}\n",
            format!("webctl update {site_name}").cyan(),
            "Update to latest IR".dimmed()
        ));
    } else {
        out.push_str("LEARN MORE\n");
        out.push_str(&format!("  webctl check {site_name}    Check for drift\n"));
        out.push_str(&format!("  webctl update {site_name}   Update to latest IR\n"));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_descriptor() -> webctl_ir::SiteDescriptor {
        webctl_ir::SiteDescriptor {
            meta: webctl_ir::SiteMeta {
                site_name: "sunat".into(),
                display_name: "SUNAT Operaciones en Linea".into(),
                source_url: "https://www.sunat.gob.pe".into(),
                ir_version: "0.1.0".into(),
            },
            provenance: webctl_ir::Provenance {
                generated_at: "2026-04-10T22:33:00Z".into(),
                technique: webctl_ir::ProvenanceTechnique::Http,
                classifier_bucket: "FormSessionLegacy".into(),
                probe_duration_sec: 639,
            },
            operations: vec![
                webctl_ir::OperationDescriptor {
                    command_path: vec!["rhe".into(), "consulta-emisor".into()],
                    summary: "Consulta RHE emitidos".into(),
                    description: "Consulta recibos por honorarios electronicos".into(),
                    operation_kind: webctl_ir::OperationKind::Read,
                    transport: webctl_ir::OperationTransport::Http(webctl_ir::HttpOperation {
                        endpoint_index: 0,
                    }),
                },
                webctl_ir::OperationDescriptor {
                    command_path: vec!["ficha-ruc".into()],
                    summary: "Consulta ficha RUC".into(),
                    description: "Consulta la ficha RUC del contribuyente".into(),
                    operation_kind: webctl_ir::OperationKind::Read,
                    transport: webctl_ir::OperationTransport::Http(webctl_ir::HttpOperation {
                        endpoint_index: 1,
                    }),
                },
            ],
            http: Some(webctl_ir::HttpSurface {
                endpoints: vec![
                    webctl_ir::HttpEndpoint {
                        namespace: vec!["rhe".into()],
                        method: webctl_ir::HttpMethod::Post,
                        path: "/ol-ti-itreciboelectronico/cpelec001Alias".into(),
                        description: "Consulta emisor".into(),
                        operation_kind: webctl_ir::OperationKind::Read,
                        sample_request_content_type: Some(
                            "application/x-www-form-urlencoded".into(),
                        ),
                        sample_response_content_type: Some("text/html".into()),
                    },
                    webctl_ir::HttpEndpoint {
                        namespace: vec!["ruc".into()],
                        method: webctl_ir::HttpMethod::Get,
                        path: "/cl-ti-itmrconsruc/consultaRuc".into(),
                        description: "Consulta ficha RUC".into(),
                        operation_kind: webctl_ir::OperationKind::Read,
                        sample_request_content_type: None,
                        sample_response_content_type: Some("text/html".into()),
                    },
                ],
            }),
            ax: None,
        }
    }

    #[test]
    fn test_help_text_generation() {
        let help = build_help_text(&sample_descriptor());
        assert!(help.contains("sunat"));
        assert!(help.contains("USAGE"));
        assert!(help.contains("rhe consulta-emisor"));
        assert!(help.contains("ficha-ruc"));
    }

    #[test]
    fn test_help_text_colored_contains_content() {
        let help = build_help_text_colored(&sample_descriptor());
        assert!(help.contains("sunat"));
        assert!(help.contains("USAGE"));
        assert!(help.contains("COMMANDS"));
    }
}
