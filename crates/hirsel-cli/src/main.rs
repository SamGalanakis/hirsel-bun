use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "hirsel",
    version = hirsel_core::version::FULL_VERSION,
    about = "Herd your AI coding agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand, Debug)]
enum Command {
    /// Start the hirsel daemon
    Serve {
        /// Port to listen on
        #[arg(long, default_value_t = 8484)]
        port: u16,

        /// Show detailed build information and exit
        #[arg(long)]
        build_info: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve { port, build_info } => {
            if build_info {
                println!("{}", hirsel_core::version::build_info());
                return;
            }
            hirsel_core::init_process_tracing("server");
            let runtime = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            if let Err(error) = runtime.block_on(hirsel_core::backend::server::start_server(port)) {
                eprintln!("Error: {error}");
                std::process::exit(1);
            }
        }
    }
}
