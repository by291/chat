use std::{collections::HashSet, process::Command};

#[test]
fn generated_schema_references_resolve_inside_the_document() {
    let output = Command::new(env!("CARGO_BIN_EXE_export_openapi"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    fn check_references(value: &serde_json::Value, document: &serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                if let Some(reference) = fields.get("$ref").and_then(serde_json::Value::as_str)
                    && let Some(pointer) = reference.strip_prefix('#')
                {
                    assert!(
                        document.pointer(pointer).is_some(),
                        "unresolved OpenAPI reference: {reference}"
                    );
                }
                for field in fields.values() {
                    check_references(field, document);
                }
            }
            serde_json::Value::Array(elements) => {
                for element in elements {
                    check_references(element, document);
                }
            }
            _ => {}
        }
    }

    check_references(&document, &document);
}

#[test]
fn generated_operation_ids_are_unique_across_features() {
    let output = Command::new(env!("CARGO_BIN_EXE_export_openapi"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut identifiers = HashSet::new();

    for path in document["paths"].as_object().unwrap().values() {
        for method in ["get", "post", "put", "patch", "delete", "head", "options"] {
            if let Some(operation) = path.get(method) {
                let identifier = operation["operationId"].as_str().unwrap();
                assert!(
                    identifiers.insert(identifier),
                    "duplicate operation ID: {identifier}"
                );
            }
        }
    }
}
