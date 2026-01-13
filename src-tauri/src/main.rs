// Hirsel - Main entry point
// Launches GUI by default, or handles CLI commands

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Try to run CLI first - if a command was provided, execute it and exit
    // If no command, run_cli returns Ok(false) and we launch the GUI
    match hirsel_lib::run_cli() {
        Ok(true) => {
            // CLI command executed successfully
            return;
        }
        Ok(false) => {
            // No CLI command - launch GUI
            hirsel_lib::run();
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
