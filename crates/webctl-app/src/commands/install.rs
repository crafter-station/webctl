use crate::cli::InstallArgs;

pub async fn run(args: InstallArgs) -> anyhow::Result<()> {
    let view = crate::install_command(args).await?;
    println!("{}", crate::render_install_success(&view));
    Ok(())
}
