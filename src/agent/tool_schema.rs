use schemars::JsonSchema;
use serde_json::Value;

pub(super) fn parameters_for<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).expect("tool schema should serialize")
}
