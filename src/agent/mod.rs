pub mod answer;
pub mod command;
pub mod sandbox;
mod scroll;
mod stream;
pub mod submit_commands;
mod tool_call_log;
mod tool_schema;

pub fn detect_os() -> String {
    sysinfo::System::name().unwrap_or(std::env::consts::OS.to_string())
}
