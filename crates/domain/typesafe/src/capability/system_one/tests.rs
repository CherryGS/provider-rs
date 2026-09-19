#![allow(clippy::expect_used, clippy::unwrap_used)]

use provider_test_support::{serve_json, serve_truncated};
use serde_json::json;

use super::*;

const INVALID_ENDPOINT: &str = "not a URL";

fn request_with(question: Question) -> Request {
    Request::new(
        "jev-latest",
        "An export failed.",
        [("check".into(), question)].into(),
    )
}

fn request() -> Request {
    request_with(Question::noul("Did the export succeed?"))
}

async fn assert_invalid(request: &Request, expected: &'static str) {
    let key = crate::SecretString::from("test-key");
    let error = evaluate_at(
        &Client::new(),
        Credentials::new(&key),
        request,
        INVALID_ENDPOINT,
    )
    .await
    .expect_err("invalid input must fail before constructing the HTTP request");
    assert!(
        matches!(error, Error::InvalidRequest(field) if field == expected),
        "{error}"
    );
    assert_eq!(error.status(), None);
    assert_eq!(error.raw_body(), None);
}

#[tokio::test]
async fn sends_mixed_questions_and_preserves_typed_answers() {
    let response_body = json!({
        "model": "jev-1.13.0",
        "answers": {
            "route": {
                "type": "choice", "choice": "retry", "confidence": 0.6,
                "probabilities": {"retry": 0.8, "review": 0.2}, "future": true
            },
            "severity": {
                "type": "score", "score": 0.75, "confidence": 0.5,
                "probabilities": {"0": 0.25, "1": 0.75},
                "legend": {"0": {"impact": "minor"}, "1": ["report unavailable", null]},
                "future": {"detail": true}
            },
            "urgent": {"type": "noul", "noul": 0.9, "future": "retained"},
            "succeeded": {"type": "noul", "noul": 0.1}
        },
        "usage": {"input_tokens": 120, "output_tokens": 24, "cached_tokens": 12},
        "future": "metadata"
    });
    let (base_url, requests) = serve_json("200 OK", response_body.to_string());
    let key = crate::SecretString::from("test-key");
    let request = Request::new(
        "jev-latest",
        json!({"event": "export failed", "attempts": 2, "deadline_today": true}),
        [
            (
                "route".into(),
                Question::choice(
                    json!({"task": "Choose a next step"}),
                    [
                        ("retry".into(), Value::Null),
                        ("review".into(), json!({"owner": "operator"})),
                    ]
                    .into(),
                ),
            ),
            (
                "severity".into(),
                Question::score(
                    json!(["Assess impact", "Use the deadline"]),
                    vec![
                        json!({"impact": "minor"}),
                        json!(["report unavailable", null]),
                    ],
                ),
            ),
            (
                "urgent".into(),
                Question::Noul {
                    instructions: "Is action urgent?".into(),
                    criteria: Some(NoulCriteria {
                        when_true: Some(json!({"deadline": "today"})),
                        when_false: Some(json!(["can wait"])),
                    }),
                },
            ),
            (
                "succeeded".into(),
                Question::noul("Did the export succeed?"),
            ),
        ]
        .into(),
    );

    let response = evaluate_at(
        &Client::new(),
        Credentials::new(&key),
        &request,
        &format!("{base_url}/v1/systemone"),
    )
    .await
    .expect("evaluation succeeds");
    assert_eq!(response.model, "jev-1.13.0");
    let Answer::Choice(choice) = &response.answers["route"] else {
        panic!("expected Choice")
    };
    assert_eq!(choice.choice, "retry");
    assert_eq!(choice.confidence, 0.6);
    assert_eq!(choice.probabilities["retry"], 0.8);
    assert_eq!(choice.probabilities["review"], 0.2);
    assert_eq!(choice.extra["future"], true);
    let Answer::Score(score) = &response.answers["severity"] else {
        panic!("expected Score")
    };
    assert_eq!(score.score, 0.75);
    assert_eq!(score.confidence, 0.5);
    assert_eq!(score.probabilities["1"], 0.75);
    assert_eq!(score.legend["0"], json!({"impact": "minor"}));
    assert_eq!(score.legend["1"], json!(["report unavailable", null]));
    let Answer::Noul(noul) = &response.answers["urgent"] else {
        panic!("expected Noul")
    };
    assert_eq!(noul.noul, 0.9);
    assert_eq!(noul.extra["future"], "retained");
    assert_eq!(response.usage.input_tokens, Some(120));
    assert_eq!(response.usage.output_tokens, Some(24));
    assert_eq!(response.usage.extra["cached_tokens"], 12);
    assert_eq!(
        serde_json::to_value(&response).expect("serialize response"),
        response_body
    );

    let captured = requests.recv().expect("captured request");
    let (headers, body) = captured.split_once("\r\n\r\n").expect("HTTP request");
    let headers = headers.to_ascii_lowercase();
    assert!(headers.starts_with("post /v1/systemone http/1.1\r\n"));
    assert!(headers.contains("\r\nauthorization: bearer test-key\r\n"));
    assert!(headers.contains("\r\ncontent-type: application/json\r\n"));
    assert!(headers.contains("\r\naccept: application/json\r\n"));
    assert!(headers.contains(&format!("\r\nuser-agent: {USER_AGENT}\r\n")));
    assert_eq!(
        serde_json::from_str::<Value>(body).expect("request JSON"),
        json!({
            "model": "jev-latest",
            "state": {"event": "export failed", "attempts": 2, "deadline_today": true},
            "questions": {
                "route": {"type": "choice", "instructions": {"task": "Choose a next step"},
                    "criteria": {"retry": null, "review": {"owner": "operator"}}},
                "severity": {"type": "score", "instructions": ["Assess impact", "Use the deadline"],
                    "criteria": [{"impact": "minor"}, ["report unavailable", null]]},
                "urgent": {"type": "noul", "instructions": "Is action urgent?",
                    "criteria": {"true": {"deadline": "today"}, "false": ["can wait"]}},
                "succeeded": {"type": "noul", "instructions": "Did the export succeed?"}
            }
        })
    );
}

#[tokio::test]
async fn supports_sdk_entry_shapes_and_unreported_token_counts() {
    let key = crate::SecretString::from("test-key");
    for (state, usage) in [
        (json!("text"), json!({})),
        (
            json!(["text", {"attempt": 1}]),
            json!({"input_tokens": null, "output_tokens": null}),
        ),
        (Value::Null, json!({"input_tokens": 0})),
    ] {
        let response_body = json!({"model": "jev-1.13.0", "answers": {"check": {"type": "noul", "noul": 0.5}}, "usage": usage});
        let (endpoint, requests) = serve_json("200 OK", response_body.to_string());
        let mut request = request_with(Question::noul(Value::Null));
        request.state = state.clone();
        let response = evaluate_at(&Client::new(), Credentials::new(&key), &request, &endpoint)
            .await
            .expect("supported entry shape");
        assert_eq!(
            response.usage.input_tokens,
            usage.get("input_tokens").and_then(Value::as_u64)
        );
        assert_eq!(response.usage.output_tokens, None);
        let captured = requests.recv().expect("captured request");
        let (_, body) = captured.split_once("\r\n\r\n").expect("HTTP request");
        let body: Value = serde_json::from_str(body).expect("request JSON");
        assert_eq!(body["state"], state);
        assert_eq!(
            body["questions"]["check"],
            json!({"type": "noul", "instructions": null})
        );
    }
}

#[tokio::test]
async fn rejects_empty_credentials_before_exchange() {
    for key in ["", " \t\r\n"] {
        let key = crate::SecretString::from(key);
        let error = evaluate_at(
            &Client::new(),
            Credentials::new(&key),
            &request(),
            INVALID_ENDPOINT,
        )
        .await
        .expect_err("blank API key");
        assert!(matches!(error, Error::InvalidCredentials));
        assert_eq!(error.status(), None);
        assert_eq!(error.raw_body(), None);
    }
}

#[tokio::test]
async fn rejects_invalid_request_fields_before_exchange() {
    let mut invalid = request();
    invalid.model = " \t".into();
    assert_invalid(&invalid, "model").await;
    invalid = request();
    invalid.questions.clear();
    assert_invalid(&invalid, "questions").await;

    for scalar in [json!(true), json!(3)] {
        invalid = request();
        invalid.state = scalar.clone();
        assert_invalid(&invalid, "state").await;
        for question in [
            Question::noul(scalar.clone()),
            Question::choice(scalar.clone(), [("yes".into(), Value::Null)].into()),
            Question::score(scalar.clone(), vec![Value::Null; 2]),
        ] {
            assert_invalid(&request_with(question), "questions.instructions").await;
        }
    }
}

#[tokio::test]
async fn enforces_criteria_limits_and_description_shapes() {
    for count in [0, 256] {
        let criteria = (0..count).map(|i| (i.to_string(), Value::Null)).collect();
        assert_invalid(
            &request_with(Question::choice("Pick one", criteria)),
            "questions.choice.criteria",
        )
        .await;
    }
    for count in [0, 1, 11] {
        assert_invalid(
            &request_with(Question::score("Rate it", vec![Value::Null; count])),
            "questions.score.criteria",
        )
        .await;
    }
    for scalar in [json!(false), json!(1)] {
        assert_invalid(
            &request_with(Question::choice(
                "Pick one",
                [("bad".into(), scalar.clone())].into(),
            )),
            "questions.choice.criteria",
        )
        .await;
        assert_invalid(
            &request_with(Question::score(
                "Rate it",
                vec![Value::Null, scalar.clone()],
            )),
            "questions.score.criteria",
        )
        .await;
        for criteria in [
            NoulCriteria {
                when_true: Some(scalar.clone()),
                when_false: None,
            },
            NoulCriteria {
                when_true: None,
                when_false: Some(scalar.clone()),
            },
        ] {
            assert_invalid(
                &request_with(Question::Noul {
                    instructions: Value::Null,
                    criteria: Some(criteria),
                }),
                "questions.noul.criteria",
            )
            .await;
        }
    }
}

#[test]
fn accepts_criteria_boundaries_and_preserves_noul_omission() {
    let key = crate::SecretString::from("test-key");
    for count in [1, 255] {
        let criteria = (0..count).map(|i| (i.to_string(), Value::Null)).collect();
        validate(
            Credentials::new(&key),
            &request_with(Question::choice(Value::Null, criteria)),
        )
        .expect("valid Choice size");
    }
    for count in [2, 10] {
        validate(
            Credentials::new(&key),
            &request_with(Question::score(Value::Null, vec![Value::Null; count])),
        )
        .expect("valid Score size");
    }
    let question = Question::Noul {
        instructions: Value::Null,
        criteria: Some(NoulCriteria {
            when_true: Some(Value::Null),
            when_false: None,
        }),
    };
    validate(Credentials::new(&key), &request_with(question.clone()))
        .expect("valid null criterion");
    assert_eq!(
        serde_json::to_value(question).expect("serialize question"),
        json!({
            "type": "noul", "instructions": null, "criteria": {"true": null}
        })
    );
}

#[tokio::test]
async fn preserves_provider_errors_without_retry_or_body_in_display() {
    let key = crate::SecretString::from("test-key");
    let body = r#"{"detail":"sensitive provider message"}"#;
    for (status, code) in [
        ("401 Unauthorized", 401),
        ("422 Unprocessable Entity", 422),
        ("429 Too Many Requests", 429),
        ("529 Overloaded", 529),
    ] {
        let (endpoint, requests) = serve_json(status, body);
        let error = evaluate_at(
            &Client::new(),
            Credentials::new(&key),
            &request(),
            &endpoint,
        )
        .await
        .expect_err("provider rejects request");
        assert!(matches!(error, Error::Response { .. }));
        assert_eq!(
            error.status(),
            Some(StatusCode::from_u16(code).expect("status code"))
        );
        assert_eq!(error.raw_body(), Some(body));
        assert!(!error.to_string().contains("sensitive provider message"));
        requests
            .recv()
            .expect("single request reached the one-shot server");
    }
}

#[tokio::test]
async fn preserves_decode_body_and_source_for_malformed_answers() {
    let key = crate::SecretString::from("test-key");
    for body in [
        "not JSON",
        r#"{"model":"jev-latest","usage":{},"answers":{"check":{"type":"noul"}}}"#,
        r#"{"model":"jev-latest","usage":{},"answers":{"check":{"type":"future","value":true}}}"#,
        r#"{"model":"jev-latest","usage":{},"answers":{"check":{"type":"choice","choice":"retry"}}}"#,
        r#"{"model":"jev-latest","usage":{}}"#,
    ] {
        let (endpoint, requests) = serve_json("200 OK", body);
        let error = evaluate_at(
            &Client::new(),
            Credentials::new(&key),
            &request(),
            &endpoint,
        )
        .await
        .expect_err("malformed response");
        assert!(matches!(error, Error::Decode { .. }));
        assert_eq!(error.raw_body(), Some(body));
        assert!(
            std::error::Error::source(&error)
                .and_then(|e| e.downcast_ref::<serde_json::Error>())
                .is_some()
        );
        requests.recv().expect("captured request");
    }
}

#[tokio::test]
async fn preserves_status_and_source_when_body_is_truncated() {
    let key = crate::SecretString::from("test-key");
    for (status, expected) in [
        ("200 OK", StatusCode::OK),
        ("429 Too Many Requests", StatusCode::TOO_MANY_REQUESTS),
    ] {
        let (endpoint, _requests) = serve_truncated(status, "application/json");
        let error = evaluate_at(
            &Client::new(),
            Credentials::new(&key),
            &request(),
            &endpoint,
        )
        .await
        .expect_err("truncated body");
        assert!(matches!(error, Error::BodyRead { .. }));
        assert_eq!(error.status(), Some(expected));
        assert_eq!(error.raw_body(), None);
        assert!(
            std::error::Error::source(&error)
                .and_then(|e| e.downcast_ref::<reqwest::Error>())
                .is_some()
        );
    }
}

#[tokio::test]
async fn preserves_exchange_error_source() {
    let key = crate::SecretString::from("test-key");
    let error = evaluate_at(
        &Client::new(),
        Credentials::new(&key),
        &request(),
        INVALID_ENDPOINT,
    )
    .await
    .expect_err("invalid URL");
    assert!(matches!(error, Error::Exchange(_)));
    assert_eq!(error.status(), None);
    assert_eq!(error.raw_body(), None);
    assert!(
        std::error::Error::source(&error)
            .and_then(|e| e.downcast_ref::<reqwest::Error>())
            .is_some()
    );
}
