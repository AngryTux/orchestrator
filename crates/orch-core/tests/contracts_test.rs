use orch_core::contracts::*;
use serde_json::json;

// ---- Scene 2.2: Serialization ----

#[test]
fn intent_serializes_minimal() {
    let intent = IntentContract {
        prompt: "what is CQRS?".into(),
        namespace: "default".into(),
        formation: None,
        isolation_profile: None,
        model: None,
        provider: None,
    };

    let json = serde_json::to_value(&intent).unwrap();
    assert_eq!(json["prompt"], "what is CQRS?");
    assert_eq!(json["namespace"], "default");
    // Optional fields should be absent when None
    assert!(json.get("formation").is_none());
    assert!(json.get("model").is_none());
}

#[test]
fn intent_serializes_with_overrides() {
    let intent = IntentContract {
        prompt: "compare Redis vs Memcached".into(),
        namespace: "secure".into(),
        formation: Some(FormationType::Duet),
        isolation_profile: Some("strict".into()),
        model: Some("claude/opus".into()),
        provider: Some("claude".into()),
    };

    let json = serde_json::to_value(&intent).unwrap();
    assert_eq!(json["formation"], "duet");
    assert_eq!(json["isolation_profile"], "strict");
}

#[test]
fn intent_deserializes_minimal() {
    let json = json!({
        "prompt": "what is CQRS?",
        "namespace": "default"
    });

    let intent: IntentContract = serde_json::from_value(json).unwrap();
    assert_eq!(intent.prompt, "what is CQRS?");
    assert!(intent.formation.is_none());
}

#[test]
fn intent_roundtrip() {
    let original = IntentContract {
        prompt: "explain microservices".into(),
        namespace: "lab".into(),
        formation: Some(FormationType::Solo),
        isolation_profile: None,
        model: None,
        provider: None,
    };

    let json = serde_json::to_string(&original).unwrap();
    let restored: IntentContract = serde_json::from_str(&json).unwrap();

    assert_eq!(original.prompt, restored.prompt);
    assert_eq!(original.namespace, restored.namespace);
    assert_eq!(original.formation, restored.formation);
}

#[test]
fn score_serializes() {
    let score = ScoreContract {
        performance_id: "perf-001".into(),
        formation: FormationType::Duet,
        sections: vec![
            Section {
                id: "sec-001".into(),
                provider: "claude".into(),
                model: "sonnet".into(),
                prompt: "analyze Redis".into(),
                depends_on: vec![],
            },
            Section {
                id: "sec-002".into(),
                provider: "claude".into(),
                model: "sonnet".into(),
                prompt: "analyze Memcached".into(),
                depends_on: vec![],
            },
        ],
    };

    let json = serde_json::to_value(&score).unwrap();
    assert_eq!(json["formation"], "duet");
    assert_eq!(json["sections"].as_array().unwrap().len(), 2);
}

#[test]
fn score_deserializes_without_depends_on() {
    let json = json!({
        "performance_id": "perf-001",
        "formation": "solo",
        "sections": [{
            "id": "sec-001",
            "provider": "claude",
            "model": "opus",
            "prompt": "explain CQRS"
        }]
    });

    let score: ScoreContract = serde_json::from_value(json).unwrap();
    assert_eq!(score.formation, FormationType::Solo);
    assert_eq!(score.sections.len(), 1);
    assert!(score.sections[0].depends_on.is_empty());
}

#[test]
fn result_serializes_success() {
    let result = ResultContract {
        workspace_id: "ws-001".into(),
        section_id: "sec-001".into(),
        provider: "claude".into(),
        model: "sonnet".into(),
        output: "Redis is an in-memory data store...".into(),
        tokens_in: 150,
        tokens_out: 500,
        cost_usd: 0.003,
        duration_ms: 2400,
        success: true,
        error: None,
    };

    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["tokens_in"], 150);
    assert_eq!(json["success"], true);
    assert!(json.get("error").is_none());
}

#[test]
fn result_serializes_failure() {
    let result = ResultContract {
        workspace_id: "ws-002".into(),
        section_id: "sec-002".into(),
        provider: "claude".into(),
        model: "sonnet".into(),
        output: String::new(),
        tokens_in: 0,
        tokens_out: 0,
        cost_usd: 0.0,
        duration_ms: 500,
        success: false,
        error: Some("rate limit exceeded".into()),
    };

    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["success"], false);
    assert_eq!(json["error"], "rate limit exceeded");
}

#[test]
fn coda_roundtrip() {
    let original = CodaContract {
        performance_id: "perf-001".into(),
        summary: "Redis is better for caching".into(),
        formation: FormationType::Duet,
        harmony: true,
        sections: vec![ResultContract {
            workspace_id: "ws-001".into(),
            section_id: "sec-001".into(),
            provider: "claude".into(),
            model: "opus".into(),
            output: "full output".into(),
            tokens_in: 200,
            tokens_out: 800,
            cost_usd: 0.01,
            duration_ms: 3000,
            success: true,
            error: None,
        }],
        total_tokens_in: 200,
        total_tokens_out: 800,
        total_cost_usd: 0.01,
        total_duration_ms: 3000,
    };

    let json = serde_json::to_string(&original).unwrap();
    let restored: CodaContract = serde_json::from_str(&json).unwrap();

    assert_eq!(original.performance_id, restored.performance_id);
    assert_eq!(original.formation, restored.formation);
    assert_eq!(original.harmony, restored.harmony);
    assert_eq!(original.sections.len(), restored.sections.len());
}

// ---- Enums ----

#[test]
fn formation_type_serializes_snake_case() {
    assert_eq!(serde_json::to_value(FormationType::Solo).unwrap(), "solo");
    assert_eq!(
        serde_json::to_value(FormationType::Chamber).unwrap(),
        "chamber"
    );
    assert_eq!(
        serde_json::to_value(FormationType::Symphonic).unwrap(),
        "symphonic"
    );
}

#[test]
fn formation_type_deserializes_snake_case() {
    let f: FormationType = serde_json::from_value(json!("duet")).unwrap();
    assert_eq!(f, FormationType::Duet);

    let f: FormationType = serde_json::from_value(json!("opera")).unwrap();
    assert_eq!(f, FormationType::Opera);
}

#[test]
fn formation_type_rejects_unknown() {
    let result = serde_json::from_value::<FormationType>(json!("octet"));
    assert!(result.is_err());
}

#[test]
fn performance_state_serializes_snake_case() {
    assert_eq!(
        serde_json::to_value(PerformanceState::Arranging).unwrap(),
        "arranging"
    );
    assert_eq!(
        serde_json::to_value(PerformanceState::Consolidating).unwrap(),
        "consolidating"
    );
    assert_eq!(
        serde_json::to_value(PerformanceState::Failed).unwrap(),
        "failed"
    );
}

// ---- Scene 2.3 + 2.4: Validation ----

#[test]
fn intent_valid_passes() {
    let intent = IntentContract {
        prompt: "what is CQRS?".into(),
        namespace: "default".into(),
        formation: None,
        isolation_profile: None,
        model: None,
        provider: None,
    };
    assert!(intent.validate().is_ok());
}

#[test]
fn intent_empty_prompt_fails() {
    let intent = IntentContract {
        prompt: String::new(),
        namespace: "default".into(),
        formation: None,
        isolation_profile: None,
        model: None,
        provider: None,
    };
    let err = intent.validate().unwrap_err();
    assert!(err.to_string().contains("prompt"));
}

#[test]
fn intent_empty_namespace_fails() {
    let intent = IntentContract {
        prompt: "test".into(),
        namespace: String::new(),
        formation: None,
        isolation_profile: None,
        model: None,
        provider: None,
    };
    let err = intent.validate().unwrap_err();
    assert!(err.to_string().contains("namespace"));
}

#[test]
fn score_valid_passes() {
    let score = ScoreContract {
        performance_id: "perf-001".into(),
        formation: FormationType::Solo,
        sections: vec![Section {
            id: "sec-001".into(),
            provider: "claude".into(),
            model: "opus".into(),
            prompt: "explain".into(),
            depends_on: vec![],
        }],
    };
    assert!(score.validate().is_ok());
}

#[test]
fn score_empty_sections_fails() {
    let score = ScoreContract {
        performance_id: "perf-001".into(),
        formation: FormationType::Solo,
        sections: vec![],
    };
    let err = score.validate().unwrap_err();
    assert!(err.to_string().contains("sections"));
}

#[test]
fn score_section_empty_provider_fails() {
    let score = ScoreContract {
        performance_id: "perf-001".into(),
        formation: FormationType::Solo,
        sections: vec![Section {
            id: "sec-001".into(),
            provider: String::new(),
            model: "opus".into(),
            prompt: "explain".into(),
            depends_on: vec![],
        }],
    };
    let err = score.validate().unwrap_err();
    assert!(err.to_string().contains("provider"));
}

#[test]
fn result_valid_passes() {
    let result = ResultContract {
        workspace_id: "ws-001".into(),
        section_id: "sec-001".into(),
        provider: "claude".into(),
        model: "opus".into(),
        output: "some output".into(),
        tokens_in: 100,
        tokens_out: 200,
        cost_usd: 0.01,
        duration_ms: 1000,
        success: true,
        error: None,
    };
    assert!(result.validate().is_ok());
}

#[test]
fn result_failed_without_error_message_fails() {
    let result = ResultContract {
        workspace_id: "ws-001".into(),
        section_id: "sec-001".into(),
        provider: "claude".into(),
        model: "opus".into(),
        output: String::new(),
        tokens_in: 0,
        tokens_out: 0,
        cost_usd: 0.0,
        duration_ms: 500,
        success: false,
        error: None,
    };
    let err = result.validate().unwrap_err();
    assert!(err.to_string().contains("error"));
}

#[test]
fn coda_valid_passes() {
    let coda = CodaContract {
        performance_id: "perf-001".into(),
        summary: "answer".into(),
        formation: FormationType::Solo,
        harmony: true,
        sections: vec![ResultContract {
            workspace_id: "ws-001".into(),
            section_id: "sec-001".into(),
            provider: "claude".into(),
            model: "opus".into(),
            output: "output".into(),
            tokens_in: 100,
            tokens_out: 200,
            cost_usd: 0.01,
            duration_ms: 1000,
            success: true,
            error: None,
        }],
        total_tokens_in: 100,
        total_tokens_out: 200,
        total_cost_usd: 0.01,
        total_duration_ms: 1000,
    };
    assert!(coda.validate().is_ok());
}

#[test]
fn coda_empty_summary_fails() {
    let coda = CodaContract {
        performance_id: "perf-001".into(),
        summary: String::new(),
        formation: FormationType::Solo,
        harmony: true,
        sections: vec![],
        total_tokens_in: 0,
        total_tokens_out: 0,
        total_cost_usd: 0.0,
        total_duration_ms: 0,
    };
    let err = coda.validate().unwrap_err();
    assert!(err.to_string().contains("summary"));
}
