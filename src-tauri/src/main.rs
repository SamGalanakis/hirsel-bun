// Hirsel - Main entry point
// Launches GUI by default, or handles CLI commands

#![cfg_attr(
    all(feature = "gui", not(debug_assertions)),
    windows_subsystem = "windows"
)]

fn main() {
    // Check if running as CLI (has subcommands) or GUI (no args)
    let args: Vec<String> = std::env::args().collect();

    // Run CLI mode if:
    // - There are arguments that look like commands (not starting with --)
    // - Or --help / --version flags are present
    let has_subcommand = args.len() > 1 && !args[1].starts_with("--");
    let wants_help = args.iter().any(|a| a == "--help" || a == "-h");
    let wants_version = args.iter().any(|a| a == "--version" || a == "-V");

    if has_subcommand || wants_help || wants_version {
        // Run CLI mode - don't initialize GUI
        let exit_code = hirsel_lib::run_cli();
        std::process::exit(exit_code);
    } else {
        // Run GUI mode (only available with gui feature)
        #[cfg(feature = "gui")]
        hirsel_lib::run();

        #[cfg(not(feature = "gui"))]
        {
            eprintln!(
                "GUI not available in this build. Use CLI commands or build with --features gui"
            );
            eprintln!("Run 'hirsel --help' for available commands.");
            std::process::exit(1);
        }
    }
}
