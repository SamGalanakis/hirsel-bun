use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "hirsel-worker",
    version = hirsel_lib::version::FULL_VERSION,
    about = "Hirsel worker runtime"
)]
struct WorkerCli {
    /// Show detailed build information
    #[arg(long)]
    build_info: bool,

    #[command(subcommand)]
    command: Option<WorkerCommand>,
}

#[derive(Subcommand, Debug)]
enum WorkerCommand {
    /// Run a long-lived worker session for a scope.
    Serve {
        #[arg(long)]
        scope_file: PathBuf,
        #[arg(long)]
        socket_path: PathBuf,
    },
}

fn main() {
    hirsel_lib::init_process_tracing("worker");

    let cli = WorkerCli::parse();
    if cli.build_info {
        println!("{}", hirsel_lib::version::build_info());
        return;
    }

    let command = cli.command.unwrap_or_else(|| {
        eprintln!("Error: missing worker command");
        std::process::exit(2);
    });

    let runtime = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let result = match command {
        WorkerCommand::Serve {
            scope_file,
            socket_path,
        } => runtime.block_on(async {
            hirsel_lib::backend::shepherd_runtime::serve_worker_session(&scope_file, &socket_path)
                .await
        }),
    };

    if let Err(error) = result {
        eprintln!("Error: {}", error);
        std::process::exit(1);
    }
}
