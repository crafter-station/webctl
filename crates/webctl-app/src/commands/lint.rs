use anyhow::Context;

use crate::cli::LintArgs;

pub fn run(args: LintArgs) -> anyhow::Result<()> {
    let descriptor = webctl_ir::read_ir(&args.ir_path)
        .with_context(|| format!("failed to read IR from {}", args.ir_path.display()))?;

    match webctl_ir::lint_ir(&descriptor) {
        Ok(()) => {
            let read_ops = descriptor.operations.iter()
                .filter(|op| matches!(op.operation_kind, webctl_ir::OperationKind::Read))
                .count();
            let write_ops = descriptor.operations.iter()
                .filter(|op| matches!(op.operation_kind, webctl_ir::OperationKind::Write))
                .count();
            let http_count = descriptor.http.as_ref().map(|h| h.endpoints.len()).unwrap_or(0);
            let ax_count = descriptor.ax.as_ref().map(|a| a.actions.len()).unwrap_or(0);

            eprintln!("✓ Valid IR: {} ({})", descriptor.meta.site_name, descriptor.meta.display_name);
            eprintln!("  Operations: {} ({} read, {} write)", descriptor.operations.len(), read_ops, write_ops);
            if http_count > 0 { eprintln!("  Endpoints:  {} HTTP", http_count); }
            if ax_count > 0 { eprintln!("  Actions:    {} AX", ax_count); }
            eprintln!("  Technique:  {:?}", descriptor.provenance.technique);
            eprintln!("  Version:    {}", descriptor.meta.ir_version);
            Ok(())
        }
        Err(errors) => {
            eprintln!("✗ Invalid IR: {}", args.ir_path.display());
            for error in &errors {
                eprintln!("  - {error}");
            }
            Err(anyhow::anyhow!("{} lint error(s) found", errors.len()))
        }
    }
}
