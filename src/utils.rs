use anyhow::Result;
use schemars::{schema_for, JsonSchema};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tiktoken_rs::{cl100k_base, get_bpe_from_model, CoreBPE};

use crate::llm_models::LLMModel;
#[allow(deprecated)]
use crate::OpenAIModels;

// Get the tokenizer given a model
#[allow(deprecated)]
#[deprecated(
    since = "0.6.1",
    note = "This function is deprecated. Please use the `get_tokenizer` function instead."
)]
pub(crate) fn get_tokenizer_old(model: &OpenAIModels) -> anyhow::Result<CoreBPE> {
    let tokenizer = get_bpe_from_model(model.as_str());
    if let Err(_error) = tokenizer {
        // Fallback to the default chat model
        cl100k_base()
    } else {
        tokenizer
    }
}

// Get the tokenizer given a model
pub(crate) fn get_tokenizer<T: LLMModel>(model: &T) -> anyhow::Result<CoreBPE> {
    let tokenizer = get_bpe_from_model(model.as_str());
    if let Err(_error) = tokenizer {
        // Fallback to the default chat model
        cl100k_base()
    } else {
        tokenizer
    }
}

/// LLMs have a tendency to wrap response Json in ```json{}```. This function sanitizes
pub(crate) fn remove_json_wrapper(json_response: &str) -> String {
    let text_no_json = json_response.replace("json\n", "");
    text_no_json.replace("```", "")
}

// This function generates a Json schema for the provided type
pub(crate) fn get_type_schema<T: JsonSchema + DeserializeOwned>() -> Result<String> {
    // Instruct the Assistant to answer with the right Json format
    // Output schema is extracted from the type parameter
    let mut schema = schema_for!(T);

    // Modify the schema for `serde_json::Value` fields globally
    fix_value_schema(&mut schema);

    // Convert the schema to a JSON value
    let mut schema_json: Value = serde_json::to_value(&schema)?;

    // Remove '$schema' and 'title' elements that are added by schema_for macro but are not needed
    if let Some(obj) = schema_json.as_object_mut() {
        obj.remove("$schema");
        obj.remove("title");
    }

    // Convert the modified JSON value back to a pretty-printed JSON string
    Ok(serde_json::to_string_pretty(&schema_json)?)
}

// The Schemars crate uses `Bool(true)` for `Value`, which essentially means "accept anything". We need to replace it with actual `Object` type
fn fix_value_schema(schema: &mut schemars::schema::RootSchema) {
    if let Some(object) = &mut schema.schema.object {
        // Iterate over mutable values in the `properties` BTreeMap
        for subschema in object.properties.values_mut() {
            // Check if the schema is `Bool(true)` (placeholder for `serde_json::Value`)
            if let schemars::schema::Schema::Bool(true) = subschema {
                // Replace `true` with a proper schema for `serde_json::Value`
                *subschema = schemars::schema::Schema::Object(schemars::schema::SchemaObject {
                    instance_type: Some(schemars::schema::InstanceType::Object.into()),
                    ..Default::default()
                });
            }
        }
    }
}

//Used internally to pick a number from range based on its % representation
pub(crate) fn map_to_range(min: u32, max: u32, target: u32) -> f32 {
    // Cap the target to the percentage range [0, 100]
    let capped_target = target.min(100);

    // Calculate the target value in the range [min, max]
    let range = max as f32 - min as f32;
    let percentage = capped_target as f32 / 100.0;
    min as f32 + (range * percentage)
}

#[cfg(test)]
mod tests {
    use schemars::schema::{InstanceType, ObjectValidation, RootSchema, Schema, SchemaObject};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    use crate::llm_models::OpenAIModels;
    use crate::utils::{fix_value_schema, get_tokenizer, get_type_schema, map_to_range};

    #[derive(JsonSchema, Serialize, Deserialize)]
    struct SimpleStruct {
        id: i32,
        name: String,
    }

    #[derive(JsonSchema, Serialize, Deserialize)]
    struct StructWithValue {
        data: serde_json::Value,
    }

    #[derive(JsonSchema, Serialize, Deserialize)]
    struct NestedStruct {
        info: SimpleStruct,
        optional_field: Option<String>,
    }

    // Tokenizer tests
    #[test]
    fn it_computes_gpt3_5_tokenization() {
        let bpe = get_tokenizer(&OpenAIModels::Gpt4_32k).unwrap();
        let tokenized: Result<Vec<_>, _> = bpe
            .split_by_token_iter("This is a test         with a lot of spaces", true)
            .collect();
        let tokenized = tokenized.unwrap();
        assert_eq!(
            tokenized,
            vec!["This", " is", " a", " test", "        ", " with", " a", " lot", " of", " spaces"]
        );
    }

    // Generating correct schema for types
    #[test]
    fn test_get_type_schema_simple_struct() {
        let schema_result = get_type_schema::<SimpleStruct>();

        assert!(
            schema_result.is_ok(),
            "Expected schema generation to succeed"
        );

        let schema_json = schema_result.unwrap();
        let schema_value: Value = serde_json::from_str(&schema_json).unwrap();

        // Verify basic structure of the schema
        assert!(
            schema_value.is_object(),
            "Expected schema to be a JSON object"
        );
        let properties = schema_value["properties"].as_object().unwrap();
        assert!(properties.contains_key("id"), "Schema should contain 'id'");
        assert!(
            properties.contains_key("name"),
            "Schema should contain 'name'"
        );
    }

    #[test]
    fn test_get_type_schema_struct_with_value() {
        let schema_result = get_type_schema::<StructWithValue>();

        assert!(
            schema_result.is_ok(),
            "Expected schema generation to succeed"
        );

        let schema_json = schema_result.unwrap();
        let schema_value: Value = serde_json::from_str(&schema_json).unwrap();

        // Verify that the `data` field has been replaced with a proper object schema
        let data_schema = &schema_value["properties"]["data"];
        assert!(
            data_schema.is_object(),
            "Expected 'data' to be a JSON object"
        );
        assert_eq!(
            data_schema["type"].as_str(),
            Some("object"),
            "Expected 'data' field to be of type 'object'"
        );
    }

    #[test]
    fn test_get_type_schema_removes_schema_and_title() {
        let schema_result = get_type_schema::<SimpleStruct>();

        assert!(
            schema_result.is_ok(),
            "Expected schema generation to succeed"
        );

        let schema_json = schema_result.unwrap();
        let schema_value: Value = serde_json::from_str(&schema_json).unwrap();

        // Ensure `$schema` and `title` are removed
        assert!(
            !schema_value.as_object().unwrap().contains_key("$schema"),
            "Schema should not contain '$schema'"
        );
        assert!(
            !schema_value.as_object().unwrap().contains_key("title"),
            "Schema should not contain 'title'"
        );
    }

    #[test]
    fn test_get_type_schema_handles_nested_struct() {
        let schema_result = get_type_schema::<NestedStruct>();

        assert!(
            schema_result.is_ok(),
            "Expected schema generation to succeed"
        );

        let schema_json = schema_result.unwrap();
        let schema_value: Value = serde_json::from_str(&schema_json).unwrap();

        // Verify nested structure
        let info_schema = &schema_value["properties"]["info"];
        assert!(
            info_schema.is_object(),
            "Expected 'info' to be a JSON object"
        );

        // Check that `info` references `SimpleStruct`
        assert!(
            info_schema.get("$ref").is_some(),
            "Expected 'info' to have a $ref to a definition"
        );

        let ref_path = info_schema["$ref"].as_str().unwrap();
        assert_eq!(ref_path, "#/definitions/SimpleStruct");

        // Verify the `SimpleStruct` definition
        let simple_struct_schema = &schema_value["definitions"]["SimpleStruct"];
        let simple_struct_properties = simple_struct_schema["properties"].as_object().unwrap();

        assert!(
            simple_struct_properties.contains_key("id"),
            "SimpleStruct schema should contain 'id'"
        );
        assert!(
            simple_struct_properties.contains_key("name"),
            "SimpleStruct schema should contain 'name'"
        );
    }

    #[test]
    fn test_get_type_schema_pretty_printed_json() {
        let schema_result = get_type_schema::<SimpleStruct>();

        assert!(
            schema_result.is_ok(),
            "Expected schema generation to succeed"
        );

        let schema_json = schema_result.unwrap();

        // Verify pretty-printed formatting by checking indentation
        assert!(
            schema_json.contains('\n'),
            "Expected pretty-printed JSON with line breaks"
        );
        assert!(
            schema_json.contains("  "),
            "Expected pretty-printed JSON with indentation"
        );
    }

    // Fixing how Value is represented in schema
    #[test]
    fn test_fix_value_schema_replaces_bool_true() {
        let mut schema = RootSchema {
            schema: SchemaObject {
                object: Some(Box::new(ObjectValidation {
                    properties: {
                        let mut map = std::collections::BTreeMap::new();
                        map.insert(
                            "test_property".to_string(),
                            Schema::Bool(true), // This should be replaced
                        );
                        map
                    },
                    ..Default::default()
                })),
                ..Default::default()
            },
            ..Default::default()
        };

        fix_value_schema(&mut schema);

        // Assert that the `Bool(true)` was replaced with a `SchemaObject`
        if let Some(object) = &schema.schema.object {
            if let Schema::Object(subschema) = object.properties.get("test_property").unwrap() {
                assert_eq!(subschema.instance_type, Some(InstanceType::Object.into()));
            } else {
                panic!("Expected Schema::Object, but found something else");
            }
        } else {
            panic!("Expected object validation in schema, but none found");
        }
    }

    #[test]
    fn test_fix_value_schema_ignores_other_schemas() {
        let mut schema = RootSchema {
            schema: SchemaObject {
                object: Some(Box::new(ObjectValidation {
                    properties: {
                        let mut map = std::collections::BTreeMap::new();
                        map.insert(
                            "test_property".to_string(),
                            Schema::Object(SchemaObject {
                                instance_type: Some(InstanceType::String.into()),
                                ..Default::default()
                            }), // This should remain unchanged
                        );
                        map
                    },
                    ..Default::default()
                })),
                ..Default::default()
            },
            ..Default::default()
        };

        fix_value_schema(&mut schema);

        // Assert that the schema with `InstanceType::String` remains unchanged
        if let Some(object) = &schema.schema.object {
            if let Schema::Object(subschema) = object.properties.get("test_property").unwrap() {
                assert_eq!(subschema.instance_type, Some(InstanceType::String.into()));
            } else {
                panic!("Expected Schema::Object, but found something else");
            }
        } else {
            panic!("Expected object validation in schema, but none found");
        }
    }

    #[test]
    fn test_fix_value_schema_handles_missing_properties() {
        let mut schema = RootSchema {
            schema: SchemaObject {
                object: Some(Box::new(ObjectValidation {
                    properties: std::collections::BTreeMap::new(), // Empty properties
                    ..Default::default()
                })),
                ..Default::default()
            },
            ..Default::default()
        };

        fix_value_schema(&mut schema);

        // Assert that the properties map is still empty
        if let Some(object) = &schema.schema.object {
            assert!(object.properties.is_empty());
        } else {
            panic!("Expected object validation in schema, but none found");
        }
    }

    #[test]
    fn test_fix_value_schema_handles_missing_object() {
        let mut schema = RootSchema {
            schema: SchemaObject {
                object: None, // No object validation
                ..Default::default()
            },
            ..Default::default()
        };

        fix_value_schema(&mut schema);

        // Assert that the schema's object field is still None
        assert!(schema.schema.object.is_none());
    }

    // Mapping % target to temperature range
    #[test]
    fn test_target_at_min() {
        assert_eq!(map_to_range(0, 100, 0), 0.0);
        assert_eq!(map_to_range(10, 20, 0), 10.0);
    }

    #[test]
    fn test_target_at_max() {
        assert_eq!(map_to_range(0, 100, 100), 100.0);
        assert_eq!(map_to_range(10, 20, 100), 20.0);
    }

    #[test]
    fn test_target_in_middle() {
        assert_eq!(map_to_range(0, 100, 50), 50.0);
        assert_eq!(map_to_range(10, 20, 50), 15.0);
        assert_eq!(map_to_range(0, 1, 50), 0.5);
    }

    #[test]
    fn test_target_out_of_bounds() {
        assert_eq!(map_to_range(0, 100, 3000), 100.0); // Cap to 100
        assert_eq!(map_to_range(0, 100, 200), 100.0); // Cap to 100
        assert_eq!(map_to_range(10, 20, 200), 20.0); // Cap to 100
    }

    #[test]
    fn test_zero_range() {
        assert_eq!(map_to_range(10, 10, 50), 10.0); // Always return min if min == max
        assert_eq!(map_to_range(5, 5, 100), 5.0); // Even at max target
    }

    #[test]
    fn test_negative_behavior_not_applicable() {
        // Not applicable for unsigned inputs but could test edge cases:
        assert_eq!(map_to_range(0, 100, 0), 0.0);
    }
}
