use serde_json::Value;
use tracing::{debug, info};

pub(super) fn log(name: &str, arguments: &Value) {
    info!("{}", format_readable(name, arguments));
    debug!(tool = name, arguments = %arguments, "Raw tool call.");
}

fn format_readable(name: &str, arguments: &Value) -> String {
    let detail = match name {
        "explore" | "elevate" => format_invocation(arguments),
        "man" => format_man(arguments),
        "tldr" => format_tldr(arguments),
        "thefuck" => string_argument(arguments, "command"),
        "answer" => format_answer(arguments),
        _ => format_unknown(arguments),
    }
    .unwrap_or_else(|| "invalid arguments".into());

    format!("{name}: {}", indent_multiline(&detail))
}

fn format_invocation(arguments: &Value) -> Option<String> {
    match arguments.get("mode")?.as_str()? {
        "shell" => string_argument(arguments, "command"),
        "program" => {
            let program = arguments.get("program")?.as_str()?;
            let args = arguments
                .get("args")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str);
            Some(
                std::iter::once(program)
                    .chain(args)
                    .map(format_command_word)
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        }
        _ => None,
    }
}

fn format_man(arguments: &Value) -> Option<String> {
    let entry = arguments.get("entry")?.as_str()?;
    match arguments.get("section").and_then(Value::as_u64) {
        Some(section) => Some(format!("{entry} (section {section})")),
        None => Some(entry.into()),
    }
}

fn format_tldr(arguments: &Value) -> Option<String> {
    let page = arguments.get("page")?.as_array()?;
    Some(
        page.iter()
            .filter_map(Value::as_str)
            .map(format_command_word)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn format_answer(arguments: &Value) -> Option<String> {
    let answer = arguments.get("answer")?;
    match answer.get("mode")?.as_str()? {
        "commands" => {
            let count = answer.get("commands")?.as_array()?.len();
            let suffix = if count == 1 { "option" } else { "options" };
            Some(format!("{count} command {suffix}"))
        }
        "text" => Some("text response".into()),
        _ => None,
    }
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

fn format_command_word(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_./:=@%+,".contains(ch))
    {
        word.into()
    } else {
        serde_json::to_string(word).unwrap_or_else(|_| "\"<invalid>\"".into())
    }
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
        let output = format_readable("explore", &json!({"command": "ls", "mode": "shell"}));

        assert_eq!(output, "explore: ls");
    }

    #[test]
    fn answer_log_reports_option_count_without_command_content() {
        let output = format_readable(
            "answer",
            &json!({
                "answer": {
                    "mode": "commands",
                    "commands": [
                        {"content": "first line\nsecond line", "desc": "first"},
                        {"content": "single line", "desc": "second"}
                    ]
                }
            }),
        );

        assert!(output.contains('2'));
        assert!(!output.contains("first line"));
    }
}
