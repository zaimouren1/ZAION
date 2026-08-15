//! Mock VLLM Server for Integration Testing
//!
//! Provides a fake VLLM server that returns realistic responses
//! for testing OPD pipeline without requiring a real GPU-backed VLLM instance.

use axum::{extract::Json, response::IntoResponse, routing::post, Router};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[derive(Debug, Deserialize, Serialize)]
pub struct MockVllmRequest {
    pub model: String,
    pub messages: Vec<MockMessage>,
    pub max_tokens: usize,
    pub temperature: f32,
    pub logprobs: bool,
    pub top_logprobs: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MockMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MockVllmResponse {
    pub id: String,
    pub choices: Vec<MockChoice>,
    pub usage: MockUsage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MockChoice {
    pub index: usize,
    pub message: MockMessage,
    pub logprobs: Option<MockLogprobs>,
    pub finish_reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MockLogprobs {
    pub content: Vec<MockTokenLogprob>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MockTokenLogprob {
    pub token: String,
    pub logprob: f32,
    pub top_logprobs: Vec<MockTopLogprob>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MockTopLogprob {
    pub token: String,
    pub logprob: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MockUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

/// Mock VLLM server for testing
pub struct MockVllmServer {
    addr: SocketAddr,
}

impl MockVllmServer {
    /// Start a mock VLLM server on a random port
    pub async fn start() -> Self {
        let app = Router::new().route("/chat/completions", post(handle_completion));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Self { addr }
    }

    /// Get the server URL
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

async fn handle_completion(Json(request): Json<MockVllmRequest>) -> impl IntoResponse {
    // Detect if this is a hint extraction request (judge prompt)
    let is_judge = request
        .messages
        .iter()
        .any(|m| m.content.contains("process reward model") || m.content.contains("boxed"));

    // Detect if this is a teacher scoring request (has hint in context)
    let is_teacher = request
        .messages
        .iter()
        .any(|m| m.content.contains("[Hint") || m.content.contains("Additional Context"));

    let response = if is_judge {
        // Mock judge response with hint
        mock_judge_response()
    } else if is_teacher {
        // Mock teacher scoring response with logprobs
        mock_teacher_response()
    } else if request.logprobs {
        // Mock student scoring response with real logprobs
        mock_student_scoring_response()
    } else {
        // Mock student response
        mock_student_response()
    };

    Json(response)
}

fn mock_judge_response() -> MockVllmResponse {
    let content = r#"The next state shows a test failure with a NameError. This reveals that the function name was incorrect.

\boxed{1}

[HINT_START]The function should be named 'fizzbuzz' not 'fizz_buzz'. Check the exact function name required by the test.[HINT_END]"#;

    MockVllmResponse {
        id: "mock-judge-1".to_string(),
        choices: vec![MockChoice {
            index: 0,
            message: MockMessage {
                role: "assistant".to_string(),
                content: content.to_string(),
            },
            logprobs: None,
            finish_reason: "stop".to_string(),
        }],
        usage: MockUsage {
            prompt_tokens: 150,
            completion_tokens: 50,
            total_tokens: 200,
        },
    }
}

fn mock_teacher_response() -> MockVllmResponse {
    let tokens = ["def", " fizzbuzz", "(", "n", "):", "\n", "    ", "return"];
    let logprobs: Vec<f32> = vec![-0.1, -0.2, -0.05, -0.15, -0.08, -0.1, -0.12, -0.18];

    let content_logprobs: Vec<MockTokenLogprob> = tokens
        .iter()
        .zip(&logprobs)
        .map(|(token, logprob)| MockTokenLogprob {
            token: token.to_string(),
            logprob: *logprob,
            top_logprobs: vec![
                MockTopLogprob {
                    token: token.to_string(),
                    logprob: *logprob,
                },
                MockTopLogprob {
                    token: format!("{}_alt", token),
                    logprob: logprob - 1.0,
                },
            ],
        })
        .collect();

    MockVllmResponse {
        id: "mock-teacher-1".to_string(),
        choices: vec![MockChoice {
            index: 0,
            message: MockMessage {
                role: "assistant".to_string(),
                content: tokens.join(""),
            },
            logprobs: Some(MockLogprobs {
                content: content_logprobs,
            }),
            finish_reason: "stop".to_string(),
        }],
        usage: MockUsage {
            prompt_tokens: 200,
            completion_tokens: 8,
            total_tokens: 208,
        },
    }
}

fn mock_student_scoring_response() -> MockVllmResponse {
    let tokens = ["def", " fizzbuzz", "(", "n", "):", "\n", "    ", "return"];
    let logprobs: Vec<f32> = vec![-0.7, -0.9, -0.4, -0.55, -0.5, -0.65, -0.6, -0.75];

    let content_logprobs: Vec<MockTokenLogprob> = tokens
        .iter()
        .zip(&logprobs)
        .map(|(token, logprob)| MockTokenLogprob {
            token: token.to_string(),
            logprob: *logprob,
            top_logprobs: vec![
                MockTopLogprob {
                    token: token.to_string(),
                    logprob: *logprob,
                },
                MockTopLogprob {
                    token: format!("{}_student_alt", token),
                    logprob: logprob - 1.0,
                },
            ],
        })
        .collect();

    MockVllmResponse {
        id: "mock-student-scoring-1".to_string(),
        choices: vec![MockChoice {
            index: 0,
            message: MockMessage {
                role: "assistant".to_string(),
                content: tokens.join(""),
            },
            logprobs: Some(MockLogprobs {
                content: content_logprobs,
            }),
            finish_reason: "stop".to_string(),
        }],
        usage: MockUsage {
            prompt_tokens: 180,
            completion_tokens: 8,
            total_tokens: 188,
        },
    }
}

fn mock_student_response() -> MockVllmResponse {
    MockVllmResponse {
        id: "mock-student-1".to_string(),
        choices: vec![MockChoice {
            index: 0,
            message: MockMessage {
                role: "assistant".to_string(),
                content: "def fizz_buzz(n):\n    return []".to_string(),
            },
            logprobs: None,
            finish_reason: "stop".to_string(),
        }],
        usage: MockUsage {
            prompt_tokens: 100,
            completion_tokens: 10,
            total_tokens: 110,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_server_starts() {
        let server = MockVllmServer::start().await;
        assert!(server.url().starts_with("http://127.0.0.1:"));
    }

    #[tokio::test]
    async fn test_mock_judge_response() {
        let server = MockVllmServer::start().await;
        let client = reqwest::Client::new();

        let request = MockVllmRequest {
            model: "test".to_string(),
            messages: vec![MockMessage {
                role: "system".to_string(),
                content: "You are a process reward model".to_string(),
            }],
            max_tokens: 100,
            temperature: 0.7,
            logprobs: false,
            top_logprobs: 0,
        };

        let response = client
            .post(format!("{}/chat/completions", server.url()))
            .json(&request)
            .send()
            .await
            .unwrap();

        assert!(response.status().is_success());

        let body: MockVllmResponse = response.json().await.unwrap();
        assert!(body.choices[0].message.content.contains("\\boxed{1}"));
        assert!(body.choices[0].message.content.contains("[HINT_START]"));
    }

    #[tokio::test]
    async fn test_mock_teacher_response() {
        let server = MockVllmServer::start().await;
        let client = reqwest::Client::new();

        let request = MockVllmRequest {
            model: "test".to_string(),
            messages: vec![MockMessage {
                role: "user".to_string(),
                content: "[Hint] Check function name".to_string(),
            }],
            max_tokens: 100,
            temperature: 0.0,
            logprobs: true,
            top_logprobs: 2,
        };

        let response = client
            .post(format!("{}/chat/completions", server.url()))
            .json(&request)
            .send()
            .await
            .unwrap();

        assert!(response.status().is_success());

        let body: MockVllmResponse = response.json().await.unwrap();
        assert!(body.choices[0].logprobs.is_some());
        let logprobs = body.choices[0].logprobs.as_ref().unwrap();
        assert!(!logprobs.content.is_empty());
        assert_eq!(logprobs.content[0].token, "def");
    }

    #[tokio::test]
    async fn test_mock_student_scoring_response_returns_logprobs() {
        let server = MockVllmServer::start().await;
        let client = reqwest::Client::new();

        let request = MockVllmRequest {
            model: "student-model".to_string(),
            messages: vec![MockMessage {
                role: "assistant".to_string(),
                content: "def fizzbuzz(n):".to_string(),
            }],
            max_tokens: 100,
            temperature: 0.0,
            logprobs: true,
            top_logprobs: 2,
        };

        let response = client
            .post(format!("{}/chat/completions", server.url()))
            .json(&request)
            .send()
            .await
            .unwrap();

        assert!(response.status().is_success());

        let body: MockVllmResponse = response.json().await.unwrap();
        let logprobs = body.choices[0]
            .logprobs
            .as_ref()
            .expect("student scoring response should include logprobs");
        assert!(!logprobs.content.is_empty());
        assert_eq!(logprobs.content[0].token, "def");
    }
}
