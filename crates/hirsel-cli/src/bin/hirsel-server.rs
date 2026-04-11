use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "hirsel-server",
    version = hirsel_core::version::FULL_VERSION,
    about = "Hirsel backend server"
)]
struct ServerCli {
    #[arg(long)]
    build_info: bool,

    #[arg(long)]
    worker_image_build_label: bool,

    #[arg(long)]
    worker_image_cargo_profile: bool,

    #[arg(long, default_value_t = 8080)]
    port: u16,
}

fn main() {
    let cli = ServerCli::parse();
    if cli.build_info {
        println!("{}", hirsel_core::version::build_info());
        return;
    }
    if cli.worker_image_build_label {
        println!(
            "{}",
            hirsel_core::backend::sandbox::worker_image_build_label()
        );
        return;
    }
    if cli.worker_image_cargo_profile {
        println!(
            "{}",
            hirsel_core::backend::sandbox::worker_image_cargo_profile()
        );
        return;
    }

    hirsel_core::init_process_tracing("server");

    let runtime = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    if let Err(error) = runtime.block_on(async { hirsel_core::backend::server::start_server(cli.port).await }) {
        eprintln!("Error: {}", error);
        std::process::exit(1);
    }
}
