//! 用于测试的伪装大模型.
use std::{collections::HashMap, net::SocketAddr, path::PathBuf};

use clap::Parser;
use howlto::openai_server::{AppState, create_app};
use serde::{Deserialize, Serialize};
use tokio::{fs, io::AsyncWriteExt};
use tracing::Level;

const DEFAULT_CONFIG_FILE: &str = "~/.config/howlto/mocker.toml";
const DEFAULT_RESPONSES: &[(&str, &str)] = &[
    ("你好", "你好, 我是一个简单的大模型服务器"),
    ("可以帮我写代码吗", "当然可以, 我可以帮你写代码"),
];

#[derive(clap::Parser)]
#[clap(about = "伪装大模型端点测试工具", long_about = None)]
struct MockerArgs {
    #[clap(short, long, help = "监听地址", default_value_t = {"127.0.0.1:12345".parse().unwrap()})]
    address: SocketAddr,
    #[clap(
        short,
        long,
        help = "TOML 配置文件路径",
        default_value = DEFAULT_CONFIG_FILE
    )]
    config: PathBuf,
}

#[derive(Deserialize, Serialize)]
struct MockerConfig {
    responses: HashMap<String, String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();

    let MockerArgs {
        config: config_path,
        address,
    } = MockerArgs::parse();
    let config_path = PathBuf::from(shellexpand::tilde(&config_path.to_string_lossy()).to_string());
    let default_config_path = PathBuf::from(shellexpand::tilde(DEFAULT_CONFIG_FILE).to_string());
    if config_path == default_config_path && !config_path.is_file() {
        // 如果是默认配置文件地址, 那么自动创建.
        fs::create_dir_all(
            config_path
                .parent()
                .ok_or(anyhow::anyhow!("无法创建配置文件目录"))?,
        )
        .await?;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&config_path)
            .await?;

        let default_config = MockerConfig {
            responses: DEFAULT_RESPONSES
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        let toml_str = toml::to_string_pretty(&default_config)?;
        f.write_all(toml_str.as_bytes()).await?;
    }
    let config: MockerConfig = toml::from_str(&String::from_utf8(fs::read(&config_path).await?)?)?;

    let state = AppState::new(config.responses);
    let app = create_app(state);
    let listener = tokio::net::TcpListener::bind(address).await.unwrap();

    println!("OpenAI 兼容服务器已启动: http://{}", address);
    println!("API 端点:");
    println!("  - GET  /health");
    println!("  - GET  /v1/models");
    println!("  - POST /v1/chat/completions");

    axum::serve(listener, app).await?;
    Ok(())
}
