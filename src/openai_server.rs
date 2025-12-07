use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, convert::Infallible, sync::Arc};
use tokio_stream::wrappers::ReceiverStream;

// OpenAI API 数据结构
#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

// 流式响应结构
#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Debug, Serialize)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

// 模型列表
#[derive(Debug, Serialize)]
pub struct ModelList {
    pub object: String,
    pub data: Vec<Model>,
}

#[derive(Debug, Serialize)]
pub struct Model {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

// 应用状态 - 存储字符串映射
#[derive(Clone)]
pub struct AppState {
    pub responses: Arc<HashMap<String, String>>,
}

impl AppState {
    pub fn new(responses: HashMap<String, String>) -> Self {
        Self {
            responses: Arc::new(responses),
        }
    }
}

// API 路由处理器
pub async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Response {
    // 从最后一条用户消息中提取关键词作为查找键
    let user_message = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();

    // 在映射中查找响应
    let response_content = state
        .responses
        .get(&user_message)
        .cloned()
        .unwrap_or_else(|| format!("未找到匹配的响应: {}", user_message));

    if req.stream {
        // 流式响应
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let model = req.model.clone();

        tokio::spawn(async move {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let id = format!("chatcmpl-{}", uuid::Uuid::new_v4());

            // 发送开始块
            let start_chunk = ChatCompletionChunk {
                id: id.clone(),
                object: "chat.completion.chunk".to_string(),
                created: timestamp,
                model: model.clone(),
                choices: vec![ChunkChoice {
                    index: 0,
                    delta: Delta {
                        role: Some("assistant".to_string()),
                        content: None,
                    },
                    finish_reason: None,
                }],
            };

            let _ = tx
                .send(Ok::<_, Infallible>(
                    Event::default().json_data(start_chunk).unwrap(),
                ))
                .await;

            // 发送内容块
            let content_chunk = ChatCompletionChunk {
                id: id.clone(),
                object: "chat.completion.chunk".to_string(),
                created: timestamp,
                model: model.clone(),
                choices: vec![ChunkChoice {
                    index: 0,
                    delta: Delta {
                        role: None,
                        content: Some(response_content),
                    },
                    finish_reason: None,
                }],
            };

            let _ = tx
                .send(Ok(Event::default().json_data(content_chunk).unwrap()))
                .await;

            // 发送结束块
            let end_chunk = ChatCompletionChunk {
                id,
                object: "chat.completion.chunk".to_string(),
                created: timestamp,
                model,
                choices: vec![ChunkChoice {
                    index: 0,
                    delta: Delta {
                        role: None,
                        content: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
            };

            let _ = tx
                .send(Ok(Event::default().json_data(end_chunk).unwrap()))
                .await;

            let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
        });

        let stream = ReceiverStream::new(rx);
        Sse::new(stream).into_response()
    } else {
        // 非流式响应
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let response = ChatCompletionResponse {
            id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            object: "chat.completion".to_string(),
            created: timestamp,
            model: req.model,
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: "assistant".to_string(),
                    content: response_content,
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            },
        };

        Json(response).into_response()
    }
}

pub async fn list_models(State(_state): State<AppState>) -> Json<ModelList> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    Json(ModelList {
        object: "list".to_string(),
        data: vec![Model {
            id: "mocker".to_string(),
            object: "model".to_string(),
            created: timestamp,
            owned_by: "howlto".to_string(),
        }],
    })
}

pub async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

// 创建应用路由
pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(state)
}
