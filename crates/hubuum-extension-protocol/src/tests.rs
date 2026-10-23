use jsonc_parser::parse_to_serde_value;
use semver::Version;
use serde_json::{json, Value};

use super::{
    CommandImplementation, ExtensionManifest, ExtensionPackKind, ExtensionResponse, ProtocolError,
    SemanticOutput, SemanticOutputShape, WorkflowStep, MANIFEST_PARSE_OPTIONS, PROTOCOL_V1,
};

const EXECUTABLE: &str = r#"{
  // JSONC comments and trailing commas are supported.
  "schema_version": 1,
  "kind": "executable",
  "name": "site-inventory",
  "version": "0.1.0",
  "requires_cli": ">=0.0.9,<0.1",
  "protocol": "hubuum-cli.extension/v1",
  "executable": "bin/site-inventory",
  "config": {
"site": {
  "type": "string",
  "required": true,
},
  },
  "commands": {
"host_show": {
  "path": ["host", "show"],
  "arguments": ["host", "show"],
  "options": {
    "identifier": {
      "kind": "string",
      "position": 1,
    },
  },
},
  },
}"#;

const PORTABLE_BODY: &str = r#"
  "config": {
"hosts_class": {
  "type": "string",
  "default": "Hosts"
}
  },
  "workflows": {
"item": {
  "inputs": {
    "item": {
      "type": "json",
      "required": true
    }
  },
  "output": {
    "shape": "values",
    "type": "json"
  },
  "steps": [
    {
      "id": "keep",
      "kind": "let",
      "expr": ".input.item"
    }
  ],
  "result": "[.input.item]"
},
"snapshot": {
  "inputs": {
    "enabled": {
      "type": "boolean",
      "default": true
    },
    "items": {
      "type": "json",
      "default": [1, 2]
    }
  },
  "output": {
    "shape": "detail",
    "type": "json"
  },
  "steps": [
    {
      "id": "hosts",
      "kind": "run",
      "run": ["object", "list"],
      "when": ".input.enabled",
      "with": {
        "class": { "config": "hosts_class" },
        "all": true
      }
    },
    {
      "id": "selected",
      "kind": "let",
      "expr": ".steps.hosts"
    },
    {
      "id": "valid",
      "kind": "assert",
      "condition": ".input.enabled == true",
      "message": "snapshot must be enabled"
    },
    {
      "id": "one",
      "kind": "call",
      "call": "item",
      "when": ".input.enabled",
      "with": { "item": "one" }
    },
    {
      "id": "many",
      "kind": "for_each",
      "items": { "input": "items" },
      "as": "item",
      "call": "item",
      "max_items": 10,
      "when": ".input.enabled"
    }
  ],
  "result": "{ hosts: .steps.hosts, selected: .steps.selected, one: .steps.one, many: .steps.many }"
}
  }
"#;

#[test]
fn parses_executable_pack_and_typed_config() {
    let manifest = ExtensionManifest::parse(EXECUTABLE).expect("manifest");
    assert_eq!(manifest.kind(), ExtensionPackKind::Executable);
    assert_eq!(manifest.protocol().expect("protocol").as_str(), PROTOCOL_V1);
    assert!(manifest.supports_cli(&Version::new(0, 0, 9)));
    assert_eq!(
        manifest.commands()[0].declaration_name().as_str(),
        "host_show"
    );
    assert_eq!(
        manifest.commands()[0].options()[0]
            .position()
            .map(|position| position.get()),
        Some(1)
    );
    assert!(matches!(
        manifest.commands()[0].implementation(),
        CommandImplementation::Executable { .. }
    ));
    assert_eq!(
        manifest
            .resolve_config(&json!({"site": "oslo"}))
            .expect("config")["site"],
        "oslo"
    );
    assert!(manifest
        .resolve_config(&json!({"site": "oslo", "extra": true}))
        .expect_err("unknown key")
        .to_string()
        .contains("unknown configuration key"));
}

#[test]
fn manifest_jsonc_does_not_enable_new_json5_syntax() {
    for source in [
        r#"{"value": .5}"#,
        r#"{"value": 1.}"#,
        r#"{"value": NaN}"#,
        r#"{"value": Infinity}"#,
        r#"{"value": "\x41"}"#,
    ] {
        assert!(parse_to_serde_value::<Value>(source, &MANIFEST_PARSE_OPTIONS).is_err());
    }
}

#[test]
fn accepts_an_editor_schema_reference() {
    let manifest = EXECUTABLE.replace(
        "  \"schema_version\": 1,",
        concat!(
            "  \"$schema\": \"../../../schemas/hubuum-extension.schema.json\",\n",
            "  \"schema_version\": 1,"
        ),
    );
    ExtensionManifest::parse(&manifest).expect("manifest with $schema");
}

#[test]
fn bundled_json_schema_is_valid_json() {
    let schema = include_str!("../../../schemas/hubuum-extension.schema.json");
    let parsed: serde_json::Value = serde_json::from_str(schema).expect("valid JSON Schema");
    assert_eq!(
        parsed["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
}

#[test]
fn parses_every_tagged_step_and_named_workflow() {
    let manifest =
        ExtensionManifest::parse(&portable_manifest(PORTABLE_BODY)).expect("portable manifest");
    assert_eq!(manifest.kind(), ExtensionPackKind::Portable);
    assert!(manifest.protocol().is_none());
    let name = manifest.commands()[0].workflow().expect("workflow name");
    let workflow = manifest.workflow(name).expect("workflow");
    assert_eq!(workflow.steps().len(), 5);
    assert!(matches!(workflow.steps()[0], WorkflowStep::Run(_)));
    assert!(matches!(workflow.steps()[1], WorkflowStep::Let(_)));
    assert!(matches!(workflow.steps()[2], WorkflowStep::Assert(_)));
    assert!(matches!(workflow.steps()[3], WorkflowStep::Call(_)));
    assert!(matches!(workflow.steps()[4], WorkflowStep::ForEach(_)));
}

#[test]
fn rejects_forward_references_cross_pack_calls_and_unbounded_iteration() {
    let forward = portable_manifest(
        r#"
  "workflows": {
"snapshot": {
  "output": { "shape": "detail", "type": "json" },
  "steps": [
    {
      "id": "first",
      "kind": "run",
      "run": ["object", "show"],
      "with": { "id": { "step": "later" } }
    },
    {
      "id": "later",
      "kind": "run",
      "run": ["object", "list"]
    }
  ],
  "result": ".steps.first"
}
  }
"#,
    );
    assert!(ExtensionManifest::parse(&forward)
        .expect_err("forward reference")
        .to_string()
        .contains("earlier step"));

    let cross_pack = PORTABLE_BODY.replace("\"call\": \"item\"", "\"call\": \"other.pack\"");
    assert!(ExtensionManifest::parse(&portable_manifest(&cross_pack))
        .expect_err("cross-pack call")
        .to_string()
        .contains("snake_case"));

    let unbounded = PORTABLE_BODY.replace("          \"max_items\": 10,\n", "");
    assert!(ExtensionManifest::parse(&portable_manifest(&unbounded))
        .expect_err("missing max_items")
        .to_string()
        .contains("max_items"));
}

#[test]
fn rejects_duplicate_steps_and_noncontiguous_positions() {
    let missing_step =
        PORTABLE_BODY.replace("          \"id\": \"many\",", "          \"id\": \"one\",");
    assert!(ExtensionManifest::parse(&portable_manifest(&missing_step))
        .expect_err("duplicate step id")
        .to_string()
        .contains("duplicate step id 'one'"));

    let skipped_position = EXECUTABLE.replace("\"position\": 1", "\"position\": 2");
    assert!(ExtensionManifest::parse(&skipped_position)
        .expect_err("non-contiguous positional order")
        .to_string()
        .contains("positions must be contiguous from 1"));
}

#[test]
fn rejects_incompatible_same_pack_bindings() {
    let invalid_call_literal = PORTABLE_BODY.replace(
        "          \"type\": \"json\",\n          \"required\": true",
        "          \"type\": \"integer\",\n          \"required\": true",
    );
    assert!(
        ExtensionManifest::parse(&portable_manifest(&invalid_call_literal))
            .expect_err("incompatible call literal")
            .to_string()
            .contains("literal does not satisfy target input 'item' type 'integer'")
    );

    let invalid_iteration = PORTABLE_BODY.replace(
        "\"items\": { \"input\": \"items\" }",
        "\"items\": { \"input\": \"enabled\" }",
    );
    assert!(
        ExtensionManifest::parse(&portable_manifest(&invalid_iteration))
            .expect_err("non-iterable input")
            .to_string()
            .contains("workflow input 'enabled' is not repeatable or JSON")
    );
}

#[test]
fn rejects_toml_json5_extensions_and_invalid_declaration_names() {
    let legacy = "schema_version = 1";
    assert!(matches!(
        ExtensionManifest::parse(legacy),
        Err(ProtocolError::InvalidManifestJsonc(_))
    ));

    let json5_extensions = [
        EXECUTABLE.replace("\"schema_version\"", "schema_version"),
        EXECUTABLE.replace("\"site-inventory\"", "'site-inventory'"),
        EXECUTABLE.replace("\"schema_version\": 1", "\"schema_version\": 0x1"),
        EXECUTABLE.replace("\"schema_version\": 1", "\"schema_version\": +1"),
        EXECUTABLE.replace("\"schema_version\": 1,", "\"schema_version\": 1"),
    ];
    for manifest in json5_extensions {
        assert!(matches!(
            ExtensionManifest::parse(&manifest),
            Err(ProtocolError::InvalidManifestJsonc(_))
        ));
    }

    let invalid_name = EXECUTABLE.replace("\"host_show\"", "\"host-show\"");
    assert!(matches!(
        ExtensionManifest::parse(&invalid_name),
        Err(ProtocolError::InvalidCommandDeclarationName(name)) if name == "host-show"
    ));
}

#[test]
fn rejects_pack_kind_mixing_and_extension_run_steps() {
    assert!(ExtensionManifest::parse(
        &EXECUTABLE.replace("\"kind\": \"executable\"", "\"kind\": \"portable\"")
    )
    .expect_err("mixed kind")
    .to_string()
    .contains("cannot declare protocol or executable"));

    let recursion = PORTABLE_BODY.replace(
        "\"run\": [\"object\", \"list\"]",
        "\"run\": [\"extension\", \"inventory\", \"snapshot\"]",
    );
    assert!(ExtensionManifest::parse(&portable_manifest(&recursion))
        .expect_err("extension recursion")
        .to_string()
        .contains("cannot invoke extension commands"));
}

#[test]
fn rejects_contradictory_defaults_and_lossy_flag_inputs() {
    let required_config = PORTABLE_BODY.replace(
        "      \"type\": \"string\",\n      \"default\": \"Hosts\"",
        "      \"type\": \"string\",\n      \"required\": true,\n      \"default\": \"Hosts\"",
    );
    assert!(
        ExtensionManifest::parse(&portable_manifest(&required_config))
            .expect_err("required config default")
            .to_string()
            .contains("required config cannot also declare a default")
    );

    let required_input = PORTABLE_BODY.replace(
        "          \"type\": \"boolean\",\n          \"default\": true",
        "          \"type\": \"boolean\",\n          \"required\": true,\n          \"default\": true",
    );
    assert!(
        ExtensionManifest::parse(&portable_manifest(&required_input))
            .expect_err("required input default")
            .to_string()
            .contains("cannot be required and also declare a default")
    );

    let true_flag = portable_manifest(PORTABLE_BODY).replace(
        "\"enabled\": { \"kind\": \"boolean\", \"long\": \"enabled\" }",
        "\"enabled\": { \"kind\": \"flag\", \"long\": \"enabled\" }",
    );
    assert!(ExtensionManifest::parse(&true_flag)
        .expect_err("true default flag")
        .to_string()
        .contains("defaults to true"));

    let repeatable_flag = portable_manifest(&PORTABLE_BODY.replace(
        "          \"type\": \"boolean\",\n          \"default\": true",
        "          \"type\": \"boolean\",\n          \"repeatable\": true",
    ))
    .replace(
        "\"enabled\": { \"kind\": \"boolean\", \"long\": \"enabled\" }",
        "\"enabled\": { \"kind\": \"flag\", \"long\": \"enabled\", \"repeatable\": true }",
    );
    assert!(ExtensionManifest::parse(&repeatable_flag)
        .expect_err("repeatable flag")
        .to_string()
        .contains("repeatable workflow input"));
}

#[test]
fn deserialization_enforces_output_and_failure_invariants() {
    use super::ExtensionFailure;
    for value in [
        json!({"shape": "rows", "value": 42}),
        json!({"shape": "lines", "value": [42]}),
        json!({"shape": "detail", "value": {}, "columns": [""]}),
        json!({"shape": "detail", "value": {}, "columns": ["id", "id"]}),
    ] {
        assert!(serde_json::from_value::<SemanticOutput>(value).is_err());
    }
    for value in [
        json!({"code": "INVALID CODE", "message": "failure"}),
        json!({"code": "failure", "message": " "}),
    ] {
        assert!(serde_json::from_value::<ExtensionFailure>(value).is_err());
    }
    let output = SemanticOutput::new(
        SemanticOutputShape::Rows,
        json!([{"id": 1}]),
        vec!["id".into()],
    )
    .unwrap();
    assert_eq!(
        serde_json::from_value::<SemanticOutput>(serde_json::to_value(&output).unwrap()).unwrap(),
        output,
    );
}

#[test]
fn validates_semantic_shapes_and_protocol_responses() {
    SemanticOutput::new(
        SemanticOutputShape::Rows,
        json!([{"name": "host-1"}]),
        vec!["name".to_string()],
    )
    .expect("rows");
    assert!(SemanticOutput::new(
        SemanticOutputShape::Rows,
        json!(["not-an-object"]),
        Vec::new(),
    )
    .is_err());

    assert!(ExtensionResponse::parse(
        r#"{"protocol":"hubuum-cli.extension/v1","status":"ok","output":{"shape":"message","value":"done","columns":[]}}"#,
    )
    .expect("response")
    .is_ok());
}

#[test]
fn reserves_management_names() {
    assert!(matches!(
        ExtensionManifest::parse(&EXECUTABLE.replace("site-inventory", "validate")),
        Err(ProtocolError::ReservedPackName(_))
    ));
}

fn portable_manifest(body: &str) -> String {
    format!(
        r#"{{
  "schema_version": 1,
  "kind": "portable",
  "name": "inventory",
  "version": "0.1.0",
  "requires_cli": ">=0.0.9,<0.1",
{body},
  "commands": {{
"snapshot": {{
  "path": ["snapshot"],
  "workflow": "snapshot",
  "options": {{
    "enabled": {{ "kind": "boolean", "long": "enabled" }},
    "items": {{ "kind": "json", "long": "items" }}
  }}
}}
  }}
}}"#
    )
}
