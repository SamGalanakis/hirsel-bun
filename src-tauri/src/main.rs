// Hirsel - Main entry point
// Launches GUI by default, or handles CLI commands

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    hirsel_lib::run();
}
