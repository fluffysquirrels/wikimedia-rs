use clap::CommandFactory;
use wikimedia::Result;

/// Generate a shell completion script and write it to stdout.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    /// The name used to run this CLI application.
    #[arg(long, default_value = "wmd")]
    command_name: String,

    /// Name of the shell to generate a completion script for.
    #[arg(long, value_enum)]
    shell: clap_complete::Shell,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let mut cmd = crate::Args::command();
    clap_complete::generate(
        args.shell,
        &mut cmd,
        &*args.command_name,
        &mut std::io::stdout());

    Ok(())
}
