use jsonschema::validator_for;
use serde_json::{Value, json};

const SCHEMA: &str = include_str!("../schema/bundle-line.schema.json");
const BATCH: &str = include_str!("../../../fixtures/demo/bundle.jsonl");

fn validator() -> jsonschema::Validator {
    let schema = serde_json::from_str(SCHEMA).expect("schema is valid JSON");
    validator_for(&schema).expect("schema is a valid JSON Schema")
}

#[test]
fn demo_bundle_lines_match_schema() {
    let validator = validator();
    let mut checked = 0;
    for line in BATCH.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).expect("fixture line is JSON");
        assert!(
            validator.is_valid(&value),
            "fixture line failed schema: {:?}",
            validator.iter_errors(&value).collect::<Vec<_>>()
        );
        checked += 1;
    }
    assert_eq!(checked, 14);
}

#[test]
fn malformed_lines_are_rejected() {
    let validator = validator();
    let good_card = json!({
        "op": "knowledge_card",
        "temp_ref": "$card_1",
        "data": {
            "id": "$card_1",
            "subject": "physics",
            "title": "card",
            "topics": [],
            "blocks": [],
            "anchor_ids": [],
            "status": "draft",
            "verification": "unchecked",
            "created_at": "2026-01-01T00:00:00.000000Z",
            "updated_at": "2026-01-01T00:00:00.000000Z"
        }
    });
    assert!(validator.is_valid(&good_card));

    let mut bad_ref = good_card.clone();
    bad_ref["temp_ref"] = json!("card_1");
    assert!(!validator.is_valid(&bad_ref));

    let mut unknown_op = good_card.clone();
    unknown_op["op"] = json!("flashcard");
    assert!(!validator.is_valid(&unknown_op));

    let mut bad_block = good_card.clone();
    bad_block["data"]["blocks"] = json!([{"type": "image", "url": "x"}]);
    assert!(validator.is_valid(&bad_block)); // block bodies are domain-validated

    let mut wrong_type = good_card.clone();
    wrong_type["data"]["topics"] = json!("physics");
    assert!(!validator.is_valid(&wrong_type));

    let mut missing_id = good_card.clone();
    missing_id["data"].as_object_mut().unwrap().remove("id");
    assert!(!validator.is_valid(&missing_id));
}
