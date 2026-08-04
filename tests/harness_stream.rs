use std::{collections::VecDeque, sync::Arc, time::Duration};

use howlto::{
    agent::answer::AnswerAgent,
    config::{AppConfig, profile::AnswerProfile},
    shell::Shell,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
    task::JoinHandle,
};

struct MockServer {
    base_url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    task: JoinHandle<()>,
}

impl MockServer {
    async fn start(responses: Vec<String>) -> Self {
        Self::start_with_status(
            responses
                .into_iter()
                .map(|body| (200, body))
                .collect(),
        )
        .await
    }

    async fn start_with_status(responses: Vec<(u16, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_task = requests.clone();
        let mut responses = VecDeque::from(responses);
        let task = tokio::spawn(async move {
            while let Some((status, response)) = responses.pop_front() {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request_body = read_request_body(&mut stream).await.unwrap();
                if let Ok(request) = serde_json::from_slice(&request_body) {
                    requests_for_task.lock().await.push(request);
                }
                let body = response.as_bytes();
                let reason = match status {
                    200 => "OK",
                    500 => "Internal Server Error",
                    _ => "Response",
                };
                let header = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len(),
                );
                stream.write_all(header.as_bytes()).await.unwrap();
                stream.write_all(body).await.unwrap();
                stream.shutdown().await.unwrap();
            }
        });
        Self {
            base_url: format!("http://{address}/v1"),
            requests,
            task,
        }
    }

    async fn requests(&self) -> Vec<Value> {
        self.requests.lock().await.clone()
    }

    async fn finish(self) {
        tokio::time::timeout(Duration::from_secs(5), self.task)
            .await
            .expect("mock server should finish")
            .unwrap();
    }
}

async fn read_request_body(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end;
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Ok(Vec::new());
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            header_end = index + 4;
            break;
        }
    }
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while bytes.len().saturating_sub(header_end) < content_length {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    Ok(bytes[header_end..header_end + content_length.min(bytes.len() - header_end)].to_vec())
}

fn sse(chunks: impl IntoIterator<Item = Value>) -> String {
    let mut output = String::new();
    for chunk in chunks {
        output.push_str("data: ");
        output.push_str(&chunk.to_string());
        output.push_str("\n\n");
    }
    output.push_str("data: [DONE]\n\n");
    output
}

fn text_response(text: &str) -> String {
    sse([json!({
        "id": "mock",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "mock",
        "choices": [{
            "index": 0,
            "delta": {"role": "assistant", "content": text},
            "finish_reason": "stop"
        }],
        "usage": null
    })])
}

fn empty_response() -> String {
    sse([json!({
        "id": "mock",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "mock",
        "choices": [{
            "index": 0,
            "delta": {"role": "assistant"},
            "finish_reason": "stop"
        }],
        "usage": null
    })])
}

fn tool_response(name: &str, arguments: &str) -> String {
    sse([
        json!({
            "id": "mock",
            "object": "chat.completion.chunk",
            "created": 0,
            "model": "mock",
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "tool_calls": [{
                    "index": 0,
                    "id": "call-1",
                    "type": "function",
                    "function": {"name": name, "arguments": arguments}
                }]},
                "finish_reason": "tool_calls"
            }],
            "usage": null
        }),
    ])
}

fn make_agent(base_url: &str) -> AnswerAgent {
    let mut config = AppConfig::default();
    config.llm.base_url = base_url.to_string();
    config.llm.api_key = "test-key".to_string();
    config.llm.model = "mock".to_string();
    config.agent.use_tool_explore = false;
    config.agent.use_tool_elevate = false;
    config.agent.answer.output_n = 3;
    AnswerAgent::builder()
        .os("test-os".to_string())
        .shell(&Shell::detect_shell())
        .profile(AnswerProfile::default())
        .config(config)
        .build()
        .unwrap()
}

#[tokio::test]
async fn plain_text_is_a_normal_final_response() {
    let server = MockServer::start(vec![text_response("final text")]).await;
    let agent = make_agent(&server.base_url);
    let response = agent
        .resolve()
        .prompt("answer plainly".to_string())
        .call()
        .await
        .unwrap();
    assert_eq!(response.final_text, "final text");
    assert!(response.commands.is_empty());
    assert!(response.messages.iter().any(|message| matches!(
        message,
        rig_core::message::Message::Assistant { .. }
    )));
    server.finish().await;
}

#[tokio::test]
async fn explicit_history_is_sent_to_the_next_completion() {
    let server = MockServer::start(vec![text_response("first answer"), text_response("second answer")]).await;
    let agent = make_agent(&server.base_url);
    let first = agent
        .resolve()
        .prompt("first question".to_string())
        .call()
        .await
        .unwrap();
    let second = agent
        .resolve()
        .prompt("second question".to_string())
        .history(first.messages.clone())
        .call()
        .await
        .unwrap();

    assert_eq!(second.final_text, "second answer");
    let requests = server.requests().await;
    assert_eq!(requests.len(), 2);
    let messages = requests[1]["messages"].as_array().unwrap();
    assert!(messages.iter().any(|message| {
        message["role"] == "assistant" && message.to_string().contains("first answer")
    }));
    server.finish().await;
}

#[tokio::test]
async fn submitted_commands_and_summary_are_both_preserved() {
    let server = MockServer::start(vec![
        tool_response(
            "submit_commands",
            r#"{"commands":[{"command":"printf ok","description":"show ok"}]}"#,
        ),
        text_response("summary"),
    ])
    .await;
    let agent = make_agent(&server.base_url);
    let response = agent
        .resolve()
        .prompt("give me a command".to_string())
        .call()
        .await
        .unwrap();
    assert_eq!(response.final_text, "summary");
    assert_eq!(response.commands.len(), 1);
    assert_eq!(response.commands[0].command, "printf ok");
    let requests = server.requests().await;
    assert_eq!(requests.len(), 2);
    let messages = requests[1]["messages"].as_array().unwrap();
    assert!(messages.iter().any(|message| message["role"] == "assistant"));
    assert!(messages.iter().any(|message| message["role"] == "tool"));
    server.finish().await;
}

#[tokio::test]
async fn invalid_tool_arguments_are_returned_and_recovered() {
    let server = MockServer::start(vec![
        tool_response("submit_commands", r#"{"commands":"not-an-array"}"#),
        text_response("recovered"),
    ])
    .await;
    let agent = make_agent(&server.base_url);
    let response = agent
        .resolve()
        .prompt("recover from bad arguments".to_string())
        .call()
        .await
        .unwrap();
    assert_eq!(response.final_text, "recovered");
    assert!(response.commands.is_empty());
    let requests = server.requests().await;
    assert_eq!(requests.len(), 2);
    assert!(requests[1]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["role"] == "tool"));
    server.finish().await;
}

#[tokio::test]
async fn unknown_tool_separator_is_repaired() {
    let server = MockServer::start(vec![
        tool_response(
            "Submit-Commands",
            r#"{"commands":[{"content":"printf repaired"}]}"#,
        ),
        text_response("repaired summary"),
    ])
    .await;
    let agent = make_agent(&server.base_url);
    let response = agent
        .resolve()
        .prompt("repair tool name".to_string())
        .call()
        .await
        .unwrap();
    assert_eq!(response.final_text, "repaired summary");
    assert_eq!(response.commands[0].command, "printf repaired");
    server.finish().await;
}

#[tokio::test]
async fn empty_primary_response_uses_no_tool_finalizer() {
    let server = MockServer::start(vec![empty_response(), text_response("finalized")]).await;
    let agent = make_agent(&server.base_url);
    let response = agent
        .resolve()
        .prompt("do not leave me blank".to_string())
        .call()
        .await
        .unwrap();
    assert_eq!(response.final_text, "finalized");
    assert!(response.commands.is_empty());
    assert_eq!(server.requests().await.len(), 2);
    server.finish().await;
}

#[tokio::test]
async fn submitted_commands_skip_finalizer_when_summary_is_empty() {
    let server = MockServer::start(vec![
        tool_response(
            "submit_commands",
            r#"{"commands":[{"command":"printf candidate"}]}"#,
        ),
        empty_response(),
    ])
    .await;
    let agent = make_agent(&server.base_url);
    let response = agent
        .resolve()
        .prompt("return a command without a summary".to_string())
        .call()
        .await
        .unwrap();
    assert!(response.final_text.is_empty());
    assert_eq!(response.commands[0].command, "printf candidate");
    assert_eq!(server.requests().await.len(), 2);
    server.finish().await;
}

#[tokio::test]
async fn second_empty_response_is_an_error() {
    let server = MockServer::start(vec![empty_response(), empty_response()]).await;
    let agent = make_agent(&server.base_url);
    let error = agent
        .resolve()
        .prompt("keep returning empty".to_string())
        .call()
        .await
        .unwrap_err();
    assert!(error.to_string().contains("empty response"));
    server.finish().await;
}

#[tokio::test]
async fn unknown_tool_feedback_is_limited_to_two_retries() {
    let server = MockServer::start(vec![
        tool_response("missing-tool", "{}"),
        tool_response("missing-tool", "{}"),
        tool_response("missing-tool", "{}"),
    ])
    .await;
    let agent = make_agent(&server.base_url);
    let error = agent
        .resolve()
        .prompt("keep using an unknown tool".to_string())
        .call()
        .await
        .unwrap_err();
    assert!(error.to_string().contains("missing-tool"));
    assert_eq!(server.requests().await.len(), 3);
    server.finish().await;
}

#[tokio::test]
async fn transient_provider_error_is_retried_without_replaying_tools() {
    let server = MockServer::start_with_status(vec![
        (500, r#"{"type":"Router.Unavailable"}"#.to_string()),
        (200, text_response("recovered after provider retry")),
    ])
    .await;
    let agent = make_agent(&server.base_url);
    let response = agent
        .resolve()
        .prompt("retry transient provider failure".to_string())
        .call()
        .await
        .unwrap();
    assert_eq!(response.final_text, "recovered after provider retry");
    assert_eq!(server.requests().await.len(), 2);
    server.finish().await;
}

#[tokio::test]
async fn provider_error_after_tool_call_retries_with_structured_history() {
    let server = MockServer::start_with_status(vec![
        (
            200,
            tool_response(
                "submit_commands",
                r#"{"commands":[{"command":"printf once"}]}"#,
            ),
        ),
        (500, r#"{"type":"Router.Unavailable"}"#.to_string()),
        (500, r#"{"type":"Router.Unavailable"}"#.to_string()),
        (500, r#"{"type":"Router.Unavailable"}"#.to_string()),
        (200, text_response("continued after retry")),
    ])
    .await;
    let agent = make_agent(&server.base_url);
    let response = agent
        .resolve()
        .prompt("continue after a command".to_string())
        .call()
        .await
        .unwrap();
    assert_eq!(response.final_text, "continued after retry");
    assert_eq!(response.commands[0].command, "printf once");
    let requests = server.requests().await;
    assert_eq!(requests.len(), 5);
    let final_messages = requests[4]["messages"].as_array().unwrap();
    assert!(final_messages.iter().any(|message| message["role"] == "tool"));
    server.finish().await;
}
