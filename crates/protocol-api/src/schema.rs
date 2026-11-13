use crate::error::ProviderError;
use serde::{Deserialize, Serialize};

/// Provider 自描述的配置 Schema。前端据此动态生成表单，禁止在 UI 里硬编码协议字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigSchema {
    pub protocol_id: String,
    pub fields: Vec<ConfigField>,
}

/// 单个配置字段的声明。`key` 一旦发布不得重命名，否则会破坏既有连接配置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    pub required: bool,
    #[serde(default)]
    pub default_value: Option<serde_json::Value>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub help: Option<String>,
    /// 是否为敏感字段（密码、私钥等）。敏感字段永远不回显、不写日志。
    #[serde(default)]
    pub secret: bool,
    /// 条件显示：仅当另一个字段等于指定值时显示。
    #[serde(default)]
    pub visible_when: Option<FieldCondition>,
    #[serde(default)]
    pub validation: Option<FieldValidation>,
}

/// 字段控件类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Password,
    Number,
    Boolean,
    Select { options: Vec<SelectOption> },
    Multiline,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldCondition {
    pub key: String,
    pub equals: serde_json::Value,
}

/// 字段校验规则，全部为可选。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldValidation {
    #[serde(default)]
    pub min_length: Option<usize>,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub min: Option<i64>,
    #[serde(default)]
    pub max: Option<i64>,
    /// 正则表达式，需能被 Rust `regex` 与浏览器同时接受（避免回溯类语法）。
    #[serde(default)]
    pub pattern: Option<String>,
}

impl ConfigSchema {
    pub fn new(protocol_id: impl Into<String>, fields: Vec<ConfigField>) -> Self {
        Self {
            protocol_id: protocol_id.into(),
            fields,
        }
    }

    pub fn field(&self, key: &str) -> Option<&ConfigField> {
        self.fields.iter().find(|f| f.key == key)
    }

    /// 校验用户输入的配置对象。
    ///
    /// 规则：
    /// 1. 必须是 JSON 对象；
    /// 2. 不允许出现 Schema 未声明的字段（防止协议注入）；
    /// 3. 必填字段必须存在且类型正确；
    /// 4. 长度、数值范围与正则规则必须满足。
    pub fn validate(&self, values: &serde_json::Value) -> Result<(), ProviderError> {
        let Some(object) = values.as_object() else {
            return Err(ProviderError::ConfigInvalid {
                field: "<root>".into(),
                message: "配置必须是 JSON 对象".into(),
            });
        };

        for key in object.keys() {
            if self.field(key).is_none() {
                return Err(ProviderError::ConfigInvalid {
                    field: key.clone(),
                    message: format!("{} 协议不支持字段 {key}", self.protocol_id),
                });
            }
        }

        for field in &self.fields {
            let value = object.get(&field.key).filter(|v| !v.is_null());
            let Some(value) = value else {
                if field.required {
                    return Err(ProviderError::ConfigInvalid {
                        field: field.key.clone(),
                        message: format!("{} 为必填项", field.label),
                    });
                }
                continue;
            };
            self.validate_value(field, value)?;
        }
        Ok(())
    }

    fn validate_value(
        &self,
        field: &ConfigField,
        value: &serde_json::Value,
    ) -> Result<(), ProviderError> {
        let invalid = |message: String| ProviderError::ConfigInvalid {
            field: field.key.clone(),
            message,
        };

        match &field.kind {
            FieldKind::Text | FieldKind::Password | FieldKind::Multiline | FieldKind::Path => {
                let text = value
                    .as_str()
                    .ok_or_else(|| invalid(format!("{} 必须是字符串", field.label)))?;
                let length = text.chars().count();
                if let Some(validation) = &field.validation {
                    if let Some(min) = validation.min_length
                        && length < min
                    {
                        return Err(invalid(format!("{} 至少 {min} 个字符", field.label)));
                    }
                    if let Some(max) = validation.max_length
                        && length > max
                    {
                        return Err(invalid(format!("{} 最多 {max} 个字符", field.label)));
                    }
                    if let Some(pattern) = &validation.pattern {
                        let regex = regex::Regex::new(pattern)
                            .map_err(|e| invalid(format!("校验规则无效：{e}")))?;
                        if !regex.is_match(text) {
                            return Err(invalid(format!("{} 格式不正确", field.label)));
                        }
                    }
                }
            }
            FieldKind::Number => {
                let number = value
                    .as_i64()
                    .or_else(|| value.as_f64().map(|f| f as i64))
                    .ok_or_else(|| invalid(format!("{} 必须是整数", field.label)))?;
                if let Some(validation) = &field.validation {
                    if let Some(min) = validation.min
                        && number < min
                    {
                        return Err(invalid(format!("{} 不能小于 {min}", field.label)));
                    }
                    if let Some(max) = validation.max
                        && number > max
                    {
                        return Err(invalid(format!("{} 不能大于 {max}", field.label)));
                    }
                }
            }
            FieldKind::Boolean => {
                if !value.is_boolean() {
                    return Err(invalid(format!("{} 必须是布尔值", field.label)));
                }
            }
            FieldKind::Select { options } => {
                let text = value
                    .as_str()
                    .ok_or_else(|| invalid(format!("{} 必须是字符串", field.label)))?;
                if !options.iter().any(|o| o.value == text) {
                    return Err(invalid(format!("{} 的取值不在允许范围内", field.label)));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> ConfigSchema {
        ConfigSchema::new(
            "ssh",
            vec![
                ConfigField {
                    key: "host".into(),
                    label: "主机".into(),
                    kind: FieldKind::Text,
                    required: true,
                    default_value: None,
                    placeholder: None,
                    help: None,
                    secret: false,
                    visible_when: None,
                    validation: Some(FieldValidation {
                        min_length: Some(1),
                        ..Default::default()
                    }),
                },
                ConfigField {
                    key: "port".into(),
                    label: "端口".into(),
                    kind: FieldKind::Number,
                    required: true,
                    default_value: Some(json!(22)),
                    placeholder: None,
                    help: None,
                    secret: false,
                    visible_when: None,
                    validation: Some(FieldValidation {
                        min: Some(1),
                        max: Some(65535),
                        ..Default::default()
                    }),
                },
                ConfigField {
                    key: "password".into(),
                    label: "密码".into(),
                    kind: FieldKind::Password,
                    required: false,
                    default_value: None,
                    placeholder: None,
                    help: None,
                    secret: true,
                    visible_when: None,
                    validation: None,
                },
            ],
        )
    }

    #[test]
    fn rejects_missing_required_field() {
        let err = schema().validate(&json!({ "port": 22 })).unwrap_err();
        assert_eq!(
            err,
            ProviderError::ConfigInvalid {
                field: "host".into(),
                message: "主机 为必填项".into()
            }
        );
    }

    #[test]
    fn rejects_unknown_field() {
        let err = schema()
            .validate(&json!({ "host": "a", "port": 22, "inject": true }))
            .unwrap_err();
        assert!(matches!(err, ProviderError::ConfigInvalid { field, .. } if field == "inject"));
    }

    #[test]
    fn rejects_out_of_range_port() {
        let err = schema()
            .validate(&json!({ "host": "a", "port": 70000 }))
            .unwrap_err();
        assert!(matches!(err, ProviderError::ConfigInvalid { field, .. } if field == "port"));
    }

    #[test]
    fn accepts_valid_config() {
        schema()
            .validate(&json!({ "host": "10.0.0.1", "port": 22, "password": "x" }))
            .expect("应通过校验");
    }
}
