pub mod shell;
pub mod agent;
pub mod config;
pub mod error;
pub mod logging;
#[cfg(feature = "mocker")]
pub mod mock_openai_server;
pub mod tui;

