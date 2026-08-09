use drill_core::{Document, MAX_PROJECT_JSON_BYTES, SCHEMA_VERSION};

#[test]
fn future_schema_is_rejected_at_the_public_boundary() {
    let document = Document::demo(4, 2);
    let mut value: serde_json::Value =
        serde_json::from_str(&document.to_json().expect("serialize fixture"))
            .expect("parse fixture JSON");
    value["schema_version"] = serde_json::json!(SCHEMA_VERSION + 1);
    let json = serde_json::to_string(&value).expect("serialize future fixture");
    assert!(Document::from_json(&json).is_err());
}

#[test]
fn oversized_json_is_rejected_before_domain_allocation() {
    let json = " ".repeat(MAX_PROJECT_JSON_BYTES + 1);
    assert!(Document::from_json(&json).is_err());
}

#[test]
fn malformed_json_never_panics() {
    let corpus = [
        "",
        "null",
        "[]",
        "{",
        r#"{"schema_version":3}"#,
        r#"{"schema_version":3,"sets":[null]}"#,
    ];
    for input in corpus {
        assert!(Document::from_json(input).is_err(), "accepted {input:?}");
    }
}
