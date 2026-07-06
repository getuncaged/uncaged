//! Thin helpers over the generated `warp_multi_agent_api` protobuf types.
//!
//! Two things live here: a short `api` alias used throughout the crate, and
//! conversions between `serde_json::Value` (how providers express tool args)
//! and `prost_types::Struct` (how Warp's `CallMCPTool.args` is typed).

pub use warp_multi_agent_api as api;

use prost_types::Struct as ProtoStruct;
use prost_types::Value as ProtoValue;
use prost_types::value::Kind;
use serde_json::Map;
use serde_json::Value as JsonValue;

/// Convert a JSON object into a protobuf `Struct`. Non-object inputs are
/// wrapped under a single `value` key so nothing is silently dropped.
pub fn json_to_struct(value: &JsonValue) -> ProtoStruct {
    match value {
        JsonValue::Object(map) => ProtoStruct {
            fields: map
                .iter()
                .map(|(k, v)| (k.clone(), json_to_proto_value(v)))
                .collect(),
        },
        other => ProtoStruct {
            fields: std::iter::once(("value".to_string(), json_to_proto_value(other))).collect(),
        },
    }
}

fn json_to_proto_value(value: &JsonValue) -> ProtoValue {
    let kind = match value {
        JsonValue::Null => Kind::NullValue(0),
        JsonValue::Bool(b) => Kind::BoolValue(*b),
        JsonValue::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        JsonValue::String(s) => Kind::StringValue(s.clone()),
        JsonValue::Array(items) => Kind::ListValue(prost_types::ListValue {
            values: items.iter().map(json_to_proto_value).collect(),
        }),
        JsonValue::Object(_) => Kind::StructValue(json_to_struct(value)),
    };
    ProtoValue { kind: Some(kind) }
}

/// Convert a protobuf `Struct` back into a JSON object.
pub fn struct_to_json(value: &ProtoStruct) -> JsonValue {
    let mut map = Map::new();
    for (k, v) in &value.fields {
        map.insert(k.clone(), proto_value_to_json(v));
    }
    JsonValue::Object(map)
}

fn proto_value_to_json(value: &ProtoValue) -> JsonValue {
    match &value.kind {
        Some(Kind::NullValue(_)) | None => JsonValue::Null,
        Some(Kind::BoolValue(b)) => JsonValue::Bool(*b),
        Some(Kind::NumberValue(n)) => serde_json::Number::from_f64(*n)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Some(Kind::StringValue(s)) => JsonValue::String(s.clone()),
        Some(Kind::ListValue(list)) => {
            JsonValue::Array(list.values.iter().map(proto_value_to_json).collect())
        }
        Some(Kind::StructValue(s)) => struct_to_json(s),
    }
}
