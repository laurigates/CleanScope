//! `CleanScope` desktop application entry point
//!
//! This binary crate provides the main entry point for the desktop application.

// Prevents additional console window on Windows in release
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() {
    clean_scope_lib::run();
}
