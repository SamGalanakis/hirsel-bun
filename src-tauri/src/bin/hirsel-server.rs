use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "hirsel-server",
    version = hirsel_lib::version::FULL_VERSION,
    about = "Hirsel backend server"
)]
struct ServerCli {
    /// Show detailed build information
    #[arg(long)]
    build_info: bool,

    /// Print the expected local worker image build label and exit
    #[arg(long)]
    worker_image_build_label: bool,

    /// Print the worker cargo profile and exit
    #[arg(long)]
    worker_image_cargo_profile: bool,

    /// Port to listen on
    #[arg(long, default_value_t = 8080)]
    port: u16,
}

fn main() {
    let cli = ServerCli::parse();
    if cli.build_info {
        println!("{}", hirsel_lib::version::build_info());
        return;
    }
    if cli.worker_image_build_label {
        println!(
            "{}",
            hirsel_lib::backend::sandbox::worker_image_build_label()
        );
        return;
    }
    if cli.worker_image_cargo_profile {
        println!(
            "{}",
            hirsel_lib::backend::sandbox::worker_image_cargo_profile()
        );
        return;
    }

    hirsel_lib::init_process_tracing("server");

    let runtime = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    if let Err(error) =
        runtime.block_on(async { hirsel_lib::backend::server::start_server(cli.port).await })
    {
        eprintln!("Error: {}", error);
        std::process::exit(1);
    }
}
