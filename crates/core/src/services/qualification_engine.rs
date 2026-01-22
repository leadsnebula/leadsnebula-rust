// Qualification engine for evaluating BuyerQualificationConfig rules
// Implements the same logic as Ruby's PulsarQualificationEngine

use crate::models::buyer_qualification_config::BuyerQualificationConfig;
use crate::models::lead::Lead;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct QualificationResult {
    pub accepted: bool,
    pub reason: String,
    pub price_multiplier: Option<f64>,
}

pub struct QualificationEngine {
    lead: Lead,
    request_type: String,
    qualification_config: Option<BuyerQualificationConfig>,
}

impl QualificationEngine {
    pub fn new(
        lead: Lead,
        request_type: String,
        qualification_config: Option<BuyerQualificationConfig>,
    ) -> Self {
        Self {
            lead,
            request_type,
            qualification_config,
        }
    }

    /// Evaluate qualification rules and return result
    #[inline(always)]
    pub fn evaluate(&self) -> QualificationResult {
        // If no config exists, default pass (accept all)
        let config = match &self.qualification_config {
            Some(c) => c,
            None => {
                #[cfg(all(feature = "tracing", debug_assertions))]
                tracing::debug!("No qualification config found - defaulting to accept");
                return QualificationResult {
                    accepted: true,
                    reason: "No qualification config found - defaulting to accept".to_string(),
                    price_multiplier: None,
                };
            }
        };

        // If config is disabled, default pass
        if !config.enabled || !config.is_active {
            #[cfg(all(feature = "tracing", debug_assertions))]
            tracing::debug!("Qualification config is disabled - defaulting to accept");
            return QualificationResult {
                accepted: true,
                reason: "Qualification config is disabled - defaulting to accept".to_string(),
                price_multiplier: None,
            };
        }

        // Filter rules based on request type
        let rules_order = self.filter_rules_by_request_type(&config.rules_order, &config.config);

        // Evaluate rules in order
        for rule_name in rules_order {
            let result = self.evaluate_rule(&rule_name, &config.config);
            if !result.accepted {
                #[cfg(all(feature = "tracing", debug_assertions))]
                tracing::debug!(
                    rule = %rule_name,
                    reason = %result.reason,
                    "Lead rejected by qualification rule"
                );
                return result;
            }
        }

        // All rules passed
        QualificationResult {
            accepted: true,
            reason: "All qualification rules passed".to_string(),
            price_multiplier: None,
        }
    }

    /// Filter rules based on request type (ping vs post)
    /// Ping: only check rules for fields available in ping requests
    /// Post/Fullpost: check all rules
    fn filter_rules_by_request_type(&self, rules_order: &[String], _config: &Value) -> Vec<String> {
        if self.request_type == "ping" {
            // For ping, only check rules for fields available in ping requests
            let ping_rules = [
                "zip_blacklist",
                "zip_whitelist",
                "monthly_bill",
                "own_home",
                "purchase_timeframe",
                "credit_rating",
                "roof_shade",
                "utility_provider",
            ];
            rules_order
                .iter()
                .filter(|rule| ping_rules.contains(&rule.as_str()))
                .cloned()
                .collect()
        } else {
            // For post and fullpost, check all rules
            rules_order.to_vec()
        }
    }

    /// Evaluate a single rule
    #[inline(always)]
    fn evaluate_rule(&self, rule_name: &str, config: &Value) -> QualificationResult {
        match rule_name {
            "zip_blacklist" => self.evaluate_zip_blacklist(),
            "zip_whitelist" => self.evaluate_zip_whitelist(),
            "own_home" => {
                let rule_config = config.get("own_home_rule").and_then(|v| v.as_object());
                self.evaluate_own_home(rule_config)
            }
            "roof_shade" => {
                let rule_config = config.get("roof_shade_rules").and_then(|v| v.as_object());
                self.evaluate_roof_shade(rule_config)
            }
            "credit_rating" => {
                let rule_config = config
                    .get("credit_rating_rules")
                    .and_then(|v| v.as_object());
                self.evaluate_credit_rating(rule_config)
            }
            "monthly_bill" => {
                let rule_config = config.get("monthly_bill_rules").and_then(|v| v.as_object());
                self.evaluate_monthly_bill(rule_config)
            }
            "utility_provider" => {
                let rule_config = config
                    .get("utility_provider_rules")
                    .and_then(|v| v.as_object());
                self.evaluate_utility_provider(rule_config)
            }
            "property_type" => {
                let rejected = config
                    .get("property_type_rejected")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                self.evaluate_property_type(&rejected)
            }
            "roof_type" => {
                let rejected = config
                    .get("roof_type_rejected")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                self.evaluate_roof_type(&rejected)
            }
            "purchase_timeframe" => {
                let rule_config = config
                    .get("purchase_timeframe_rules")
                    .and_then(|v| v.as_object());
                self.evaluate_purchase_timeframe(rule_config)
            }
            _ => QualificationResult {
                accepted: true,
                reason: format!("Unknown rule: {}", rule_name),
                price_multiplier: None,
            },
        }
    }

    /// Get a value from lead's vertical_data
    fn get_value(&self, field: &str) -> Option<Value> {
        self.lead.vertical_data.get(field).cloned()
    }

    /// Get a string value from lead's vertical_data
    fn get_string_value(&self, field: &str) -> Option<String> {
        self.get_value(field).and_then(|v| {
            if v.is_string() {
                v.as_str().map(|s| s.to_string())
            } else if v.is_boolean() {
                Some(v.as_bool().unwrap().to_string())
            } else if v.is_number() {
                v.as_f64().map(|n| n.to_string())
            } else {
                None
            }
        })
    }

    /// Evaluate zip blacklist (simplified - would need DB lookup for full implementation)
    #[inline(always)]
    fn evaluate_zip_blacklist(&self) -> QualificationResult {
        let zip = self.get_string_value("zip");
        if zip.is_none() {
            return QualificationResult {
                accepted: true,
                reason: "Zip code not provided".to_string(),
                price_multiplier: None,
            };
        }
        // TODO: Implement full blacklist check with DB lookup
        // For now, default to accept
        QualificationResult {
            accepted: true,
            reason: "Zip blacklist check passed (not implemented)".to_string(),
            price_multiplier: None,
        }
    }

    /// Evaluate zip whitelist (simplified - would need DB lookup for full implementation)
    #[inline(always)]
    fn evaluate_zip_whitelist(&self) -> QualificationResult {
        let zip = self.get_string_value("zip");
        if zip.is_none() {
            return QualificationResult {
                accepted: true,
                reason: "Zip code not provided".to_string(),
                price_multiplier: None,
            };
        }
        // TODO: Implement full whitelist check with DB lookup
        // For now, default to accept
        QualificationResult {
            accepted: true,
            reason: "Zip whitelist check passed (not implemented)".to_string(),
            price_multiplier: None,
        }
    }

    /// Evaluate own_home rule
    #[inline(always)]
    fn evaluate_own_home(
        &self,
        rule_config: Option<&serde_json::Map<String, Value>>,
    ) -> QualificationResult {
        let rules = rule_config
            .and_then(|c| c.get("rules"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if rules.is_empty() {
            return QualificationResult {
                accepted: true,
                reason: "No own_home rules configured - accepting by default".to_string(),
                price_multiplier: None,
            };
        }

        let own_home = self.get_value("own_home");
        if own_home.is_none() {
            return QualificationResult {
                accepted: true,
                reason: "Own home field missing - accepting by default".to_string(),
                price_multiplier: None,
            };
        }

        // Normalize own_home value
        let own_home_str = if let Some(v) = &own_home {
            if v.is_boolean() {
                if v.as_bool().unwrap_or(false) {
                    "yes".to_string()
                } else {
                    "no".to_string()
                }
            } else {
                v.as_str().unwrap_or("").to_lowercase()
            }
        } else {
            return QualificationResult {
                accepted: true,
                reason: "Own home field missing - accepting by default".to_string(),
                price_multiplier: None,
            };
        };

        // Find matching rule
        let matching_rule = rules.iter().find(|rule| {
            if let Some(rule_value) = rule.get("value").and_then(|v| v.as_str()) {
                let rule_value_lower = rule_value.to_lowercase();
                let rule_value_normalized = match rule_value_lower.as_str() {
                    "yes" | "true" => "yes",
                    "no" | "false" => "no",
                    other => other,
                };
                rule_value_normalized == own_home_str.as_str()
            } else {
                false
            }
        });

        if matching_rule.is_none() {
            return QualificationResult {
                accepted: true,
                reason: format!("No rule configured for own_home value: {}", own_home_str),
                price_multiplier: None,
            };
        }

        let action = matching_rule
            .and_then(|r| r.get("action"))
            .and_then(|v| v.as_str())
            .unwrap_or("accept")
            .to_lowercase();

        if action == "reject" {
            return QualificationResult {
                accepted: false,
                reason: format!(
                    "Lead does not meet own_home requirement (value: {})",
                    own_home_str
                ),
                price_multiplier: None,
            };
        }

        QualificationResult {
            accepted: true,
            reason: "Own home requirement met".to_string(),
            price_multiplier: None,
        }
    }

    /// Evaluate roof_shade rule
    #[inline(always)]
    fn evaluate_roof_shade(
        &self,
        rule_config: Option<&serde_json::Map<String, Value>>,
    ) -> QualificationResult {
        if rule_config.is_none() || rule_config.unwrap().is_empty() {
            return QualificationResult {
                accepted: true,
                reason: "No roof_shade rules configured - accepting by default".to_string(),
                price_multiplier: None,
            };
        }

        let roof_shade = self.get_string_value("roof_shade");
        if roof_shade.is_none() {
            return QualificationResult {
                accepted: true,
                reason: "Roof shade field missing - accepting by default".to_string(),
                price_multiplier: None,
            };
        }

        let roof_shade_str = roof_shade.unwrap();
        let rule = rule_config.and_then(|c| c.get(&roof_shade_str));

        if rule.is_none() {
            return QualificationResult {
                accepted: true,
                reason: format!("No rule configured for roof shade: {}", roof_shade_str),
                price_multiplier: None,
            };
        }

        let action = rule
            .and_then(|r| r.get("action"))
            .and_then(|v| v.as_str())
            .unwrap_or("accept");

        if action == "reject" {
            return QualificationResult {
                accepted: false,
                reason: format!("Roof shade {} is not acceptable", roof_shade_str),
                price_multiplier: None,
            };
        }

        QualificationResult {
            accepted: true,
            reason: format!("Roof shade {} is acceptable", roof_shade_str),
            price_multiplier: None,
        }
    }

    /// Evaluate credit_rating rule
    #[inline(always)]
    fn evaluate_credit_rating(
        &self,
        rule_config: Option<&serde_json::Map<String, Value>>,
    ) -> QualificationResult {
        let rules = rule_config
            .and_then(|c| c.get("rules"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if rules.is_empty() {
            return QualificationResult {
                accepted: true,
                reason: "No credit_rating rules configured - accepting by default".to_string(),
                price_multiplier: None,
            };
        }

        let credit_rating = self.get_string_value("credit_rating");
        if credit_rating.is_none() {
            return QualificationResult {
                accepted: true,
                reason: "Credit rating field missing - accepting by default".to_string(),
                price_multiplier: None,
            };
        }

        let credit_rating_str = credit_rating.unwrap().to_lowercase();

        let matching_rule = rules.iter().find(|rule| {
            if let Some(rule_value) = rule.get("value").and_then(|v| v.as_str()) {
                rule_value.to_lowercase() == credit_rating_str
            } else {
                false
            }
        });

        if matching_rule.is_none() {
            return QualificationResult {
                accepted: true,
                reason: format!(
                    "No rule configured for credit rating value: {}",
                    credit_rating_str
                ),
                price_multiplier: None,
            };
        }

        let action = matching_rule
            .and_then(|r| r.get("action"))
            .and_then(|v| v.as_str())
            .unwrap_or("accept")
            .to_lowercase();

        if action == "reject" {
            return QualificationResult {
                accepted: false,
                reason: format!("Credit rating {} is not acceptable", credit_rating_str),
                price_multiplier: None,
            };
        }

        let price_multiplier = matching_rule
            .and_then(|r| r.get("price_multiplier"))
            .and_then(|v| v.as_f64());

        QualificationResult {
            accepted: true,
            reason: format!("Credit rating {} is acceptable", credit_rating_str),
            price_multiplier,
        }
    }

    /// Evaluate monthly_bill rule
    #[inline(always)]
    fn evaluate_monthly_bill(
        &self,
        _rule_config: Option<&serde_json::Map<String, Value>>,
    ) -> QualificationResult {
        // TODO: Implement monthly_bill evaluation
        QualificationResult {
            accepted: true,
            reason: "Monthly bill check passed (not implemented)".to_string(),
            price_multiplier: None,
        }
    }

    /// Evaluate utility_provider rule
    #[inline(always)]
    fn evaluate_utility_provider(
        &self,
        _rule_config: Option<&serde_json::Map<String, Value>>,
    ) -> QualificationResult {
        // TODO: Implement utility_provider evaluation
        QualificationResult {
            accepted: true,
            reason: "Utility provider check passed (not implemented)".to_string(),
            price_multiplier: None,
        }
    }

    /// Evaluate property_type rule
    #[inline(always)]
    fn evaluate_property_type(&self, rejected: &[String]) -> QualificationResult {
        if rejected.is_empty() {
            return QualificationResult {
                accepted: true,
                reason: "No property_type rejection rules configured - accepting by default"
                    .to_string(),
                price_multiplier: None,
            };
        }

        let property_type = self.get_string_value("property_type");
        if property_type.is_none() {
            return QualificationResult {
                accepted: true,
                reason: "Property type field missing - accepting by default".to_string(),
                price_multiplier: None,
            };
        }

        let property_type_str = property_type.unwrap().to_lowercase();
        if rejected
            .iter()
            .any(|r| r.to_lowercase() == property_type_str)
        {
            return QualificationResult {
                accepted: false,
                reason: format!("Property type {} is rejected", property_type_str),
                price_multiplier: None,
            };
        }

        QualificationResult {
            accepted: true,
            reason: format!("Property type {} is acceptable", property_type_str),
            price_multiplier: None,
        }
    }

    /// Evaluate roof_type rule
    #[inline(always)]
    fn evaluate_roof_type(&self, rejected: &[String]) -> QualificationResult {
        if rejected.is_empty() {
            return QualificationResult {
                accepted: true,
                reason: "No roof_type rejection rules configured - accepting by default"
                    .to_string(),
                price_multiplier: None,
            };
        }

        let roof_type = self.get_string_value("roof_type");
        if roof_type.is_none() {
            return QualificationResult {
                accepted: true,
                reason: "Roof type field missing - accepting by default".to_string(),
                price_multiplier: None,
            };
        }

        let roof_type_str = roof_type.unwrap().to_lowercase();
        if rejected.iter().any(|r| r.to_lowercase() == roof_type_str) {
            return QualificationResult {
                accepted: false,
                reason: format!("Roof type {} is rejected", roof_type_str),
                price_multiplier: None,
            };
        }

        QualificationResult {
            accepted: true,
            reason: format!("Roof type {} is acceptable", roof_type_str),
            price_multiplier: None,
        }
    }

    /// Evaluate purchase_timeframe rule
    fn evaluate_purchase_timeframe(
        &self,
        _rule_config: Option<&serde_json::Map<String, Value>>,
    ) -> QualificationResult {
        // TODO: Implement purchase_timeframe evaluation
        QualificationResult {
            accepted: true,
            reason: "Purchase timeframe check passed (not implemented)".to_string(),
            price_multiplier: None,
        }
    }
}
