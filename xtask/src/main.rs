use clap::Parser;
use xshell::cmd;

#[derive(Debug, clap::Args)]
struct BuildArgs {
    #[arg(long, short)]
    release: bool,

    #[arg(long, short = 'j', default_value = "0")]
    num_threads: u64,

    #[arg(long)]
    offline: bool,

    #[arg(long)]
    target: Option<String>,
}

#[derive(Debug, clap::Args)]
struct RunArgs {
    #[command(flatten)]
    build_args: BuildArgs,

    #[arg(short, long, default_value = "127.0.0.1")]
    address: String,

    #[arg(short, long, default_value = "8080")]
    port: u16,

    #[arg(long)]
    provision: bool,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    Build(BuildArgs),
    Frontend(BuildArgs),
    Run(RunArgs),
}

#[derive(Debug, clap::Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

fn build_frontend(args: &BuildArgs) -> anyhow::Result<()> {
    let release = args.release.then_some("--release");
    let offline = args.offline.then_some("--offline");
    let shell = xshell::Shell::new()?;
    {
        let _dir = shell.push_dir("leap-site");
        cmd!(shell, "trunk build {offline...} {release...}").run()?;
    }

    {
        let _dir = shell.push_dir("leap-provision-site");
        cmd!(shell, "trunk build {offline...} {release...}").run()?;
    }

    Ok(())
}

fn build(args: &BuildArgs) -> anyhow::Result<()> {
    let release = args.release.then_some("--release");
    let offline = args.offline.then_some("--offline");
    let num_threads = &(args.num_threads > 0).then_some(format!("-j={}", args.num_threads));
    let target = &args
        .target
        .as_ref()
        .map(|target| format!("--target={target}"));

    build_frontend(args)?;

    let shell = xshell::Shell::new()?;
    cmd!(
        shell,
        "cargo build {offline...} {release...} {num_threads...} {target...}"
    )
    .run()?;

    Ok(())
}

fn run(args: &RunArgs) -> anyhow::Result<()> {
    build(&args.build_args)?;

    let address = &args.address;
    let port = format!("{}", args.port);

    let release = args.build_args.release.then_some("--release");
    let provision = args.provision.then_some("--provision");
    let shell = xshell::Shell::new()?;
    cmd!(
        shell,
        "cargo run {release...} --bin leap-server -- --config ./docs/leap_config_template.toml --address {address} --port {port} {provision...}"
    )
    .run()?;

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match &args.command {
        Command::Build(args) => build(args)?,
        Command::Frontend(args) => build_frontend(args)?,
        Command::Run(args) => run(args)?,
    }

    Ok(())
}
