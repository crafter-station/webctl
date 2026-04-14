use anyhow::Context;

use crate::cli::LintArgs;

pub fn run(args: LintArgs) -> anyhow::Result<()> {
    let descriptor = webctl_ir::read_ir(&args.ir_path)
        .with_context(|| format!("failed to read IR from {}", args.ir_path.display()))?;

    match webctl_ir::lint_ir(&descriptor) {
        Ok(()) => {
            println!("IR valid");
            Ok(())
        }
        Err(errors) => {
            for error in errors {
                println!("{error}");
            }
            Err(anyhow::anyhow!("IR invalid"))
        }
    }
}
