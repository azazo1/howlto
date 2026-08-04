use serde_json::Value;
use tracing::{debug, info};

pub(super) fn log(name: &str, arguments: &Value) {
    info!("{}", format_readable(name, arguments));
    debug!(tool = name, arguments = %arguments, "Raw tool call.");
}

fn format_readable(name: &str, arguments: &Value) -> String {
    let detail = match name {
        "explore" | "elevate" => string_argument(arguments, "command")
            .or_else(|| string_argument(arguments, "cmd")),
        "submit_commands" => format_submission(arguments),
        _ => format_unknown(arguments),
    }
    .unwrap_or_else(|| "invalid arguments".into());

    format!("{name}: {}", indent_multiline(&detail))
}

fn format_submission(arguments: &Value) -> Option<String> {
    let count = arguments.get("commands")?.as_array()?.len();
    let suffix = if count == 1 { "candidate" } else { "candidates" };
    Some(format!("{count} command {suffix}"))
}

fn format_unknown(arguments: &Value) -> Option<String> {
    match arguments {
        Value::Object(fields) => Some(format!("{} arguments", fields.len())),
        Value::Array(items) => Some(format!("{} values", items.len())),
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null => Some("no arguments".into()),
    }
}

fn string_argument(arguments: &Value, name: &str) -> Option<String> {
    arguments.get(name)?.as_str().map(str::to_owned)
}

fn indent_multiline(value: &str) -> String {
    value.lines().enumerate().fold(
        String::new(),
        |mut output, (index, line)| {
            if index > 0 {
                output.push_str("\n  ");
            }
            output.push_str(line);
            output
        },
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::format_readable;

    #[test]
    fn shell_invocation_is_logged_as_command() {
        let output = format_readable("explore", &json!({"command": "ls"}));

        assert_eq!(output, "explore: ls");
    }

    #[test]
    fn submission_log_reports_option_count_without_command_content() {
        let output = format_readable(
            "submit_commands",
            &json!({
                "commands": [
                    {"command": "first line\nsecond line", "description": "first"},
                    {"command": "single line", "description": "second"}
                ]
            }),
        );

        assert!(output.contains('2'));
        assert!(!output.contains("first line"));
    }
}
