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
use tracing::info;

// OpenAI API 数据结构
#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    ContentParts(Vec<ContentPart>),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ContentPart {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ImageUrl>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
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

// 工具定义结构
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Tool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FunctionDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum ToolChoice {
    String(String),
    Object { #[serde(rename = "type")] tool_type: String, function: FunctionName },
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FunctionName {
    pub name: String,
}

// 工具调用结构
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Serialize)]
pub struct ToolCallDelta {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub tool_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionCallDelta>,
}

#[derive(Debug, Serialize)]
pub struct FunctionCallDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
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
        .and_then(|m| {
            m.content.as_ref().map(|c| match c {
                MessageContent::Text(text) => text.clone(),
                MessageContent::ContentParts(parts) => {
                    // 合并所有文本部分
                    parts
                        .iter()
                        .filter_map(|p| p.text.clone())
                        .collect::<Vec<_>>()
                        .join(" ")
                }
            })
        })
        .unwrap_or_default();
    info!(target: "mock-openai-server", "User prompt: {}", user_message);

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
                        tool_calls: None,
                    },
                    finish_reason: None,
                }],
            };

            let _ = tx
                .send(Ok::<_, Infallible>(
                    Event::default().json_data(start_chunk).unwrap(),
                ))
                .await;

            // 将响应内容按每5个字符分割发送
            let chars: Vec<char> = response_content.chars().collect();
            for chunk in chars.chunks(5) {
                let chunk_text: String = chunk.iter().collect();
                let content_chunk = ChatCompletionChunk {
                    id: id.clone(),
                    object: "chat.completion.chunk".to_string(),
                    created: timestamp,
                    model: model.clone(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: Delta {
                            role: None,
                            content: Some(chunk_text),
                            tool_calls: None,
                        },
                        finish_reason: None,
                    }],
                };

                let _ = tx
                    .send(Ok(Event::default().json_data(content_chunk).unwrap()))
                    .await;

                // 添加短暂延迟以模拟真实流式输出
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }

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
                        tool_calls: None,
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
                    content: Some(MessageContent::Text(response_content)),
                    tool_calls: None,
                    tool_call_id: None,
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
