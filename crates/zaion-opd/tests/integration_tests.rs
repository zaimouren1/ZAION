//! Integration tests for OPD pipeline with mock VLLM server
//!
//! These tests verify the complete OPD flow end-to-end:
//! 1. Extract turn pairs from conversation
//! 2. Extract hints via mock LLM judge
//! 3. Build enhanced prompts
//! 4. Score with mock teacher model
//! 5. Generate distill_token_ids / distill_logprobs

#[path = "../src/mock_vllm_server.rs"]
mod mock_vllm_server;

#[cfg(test)]
mod integration_tests {
    use super::mock_vllm_server::MockVllmServer;
    use zaion_opd::hint_extractor::{HintExtractor, HintExtractorConfig};
    use zaion_opd::opd_pipeline::{OpdPipeline, OpdPipelineConfig};
    use zaion_opd::turn_pair_parser::{ConversationMessage, TurnPairParser, TurnPairParserConfig};

    #[tokio::test]
    async fn test_hint_extractor_with_mock_server() {
        // Start mock VLLM server
        let server = MockVllmServer::start().await;

        // Configure hint extractor to use mock server
        let config = HintExtractorConfig {
            judge_model_url: server.url(),
            judge_model_name: "mock-judge".to_string(),
            num_votes: 3,
            max_next_state_chars: 2000,
        };

        let extractor = HintExtractor::new(config);

        // Test hint extraction
        let assistant_text = "def fizz_buzz(n):\n    return []";
        let next_state_text = "NameError: name 'fizzbuzz' is not defined";
        let next_state_role = "tool";

        let result = extractor
            .extract_hint(assistant_text, next_state_text, next_state_role)
            .await
            .unwrap();

        // Verify hint was extracted
        assert_eq!(result.score, 1);
        assert!(result.hint.is_some());
        let hint = result.hint.unwrap();
        assert!(hint.contains("fizzbuzz") || hint.contains("function name"));
        // Note: votes may be less than 3 if some requests fail/timeout
        assert!(result.votes >= 1 && result.votes <= 3);
        assert!(result.confidence > 0.0);
    }

    #[tokio::test]
    async fn test_opd_pipeline_end_to_end() {
        // Start mock VLLM server
        let server = MockVllmServer::start().await;

        // Configure OPD pipeline
        let config = OpdPipelineConfig {
            hint_config: HintExtractorConfig {
                judge_model_url: server.url(),
                judge_model_name: "mock-judge".to_string(),
                num_votes: 3,
                max_next_state_chars: 2000,
            },
            parser_config: TurnPairParserConfig::default(),
            teacher_model_url: server.url(),
            teacher_model_name: "mock-teacher".to_string(),
            distill_topk: 10,
            max_tokens: 2048,
        };

        let pipeline = OpdPipeline::new(config);

        // Create a realistic conversation with tool interaction
        let messages = vec![
            ConversationMessage {
                role: "user".to_string(),
                content: "Write a fizzbuzz function".to_string(),
            },
            ConversationMessage {
                role: "assistant".to_string(),
                content: "def fizz_buzz(n):\n    return []".to_string(),
            },
            ConversationMessage {
                role: "tool".to_string(),
                content: "Test failed: NameError: name 'fizzbuzz' is not defined".to_string(),
            },
        ];

        // Mock student tokens (simplified)
        let student_tokens: Vec<i32> = (0..20).collect();

        // Process sequence
        let result = pipeline
            .process_sequence(&messages, &student_tokens)
            .await
            .unwrap();

        // Verify results
        assert_eq!(result.num_turn_pairs, 1);
        assert_eq!(result.num_hints, 1);
        assert!(result.avg_hint_confidence > 0.5);
        assert_eq!(result.distill_token_ids.len(), 20);
        assert_eq!(result.distill_logprobs.len(), 20);

        // Verify distill arrays are populated (not all zeros)
        let has_nonzero_ids = result
            .distill_token_ids
            .iter()
            .any(|row| row.iter().any(|&id| id != 0));
        assert!(
            has_nonzero_ids,
            "distill_token_ids should have non-zero values"
        );
    }

    #[tokio::test]
    async fn test_opd_pipeline_no_hints() {
        // Start mock VLLM server
        let server = MockVllmServer::start().await;

        let config = OpdPipelineConfig {
            hint_config: HintExtractorConfig {
                judge_model_url: server.url(),
                judge_model_name: "mock-judge".to_string(),
                num_votes: 1,
                max_next_state_chars: 2000,
            },
            parser_config: TurnPairParserConfig::default(),
            teacher_model_url: server.url(),
            teacher_model_name: "mock-teacher".to_string(),
            distill_topk: 10,
            max_tokens: 2048,
        };

        let pipeline = OpdPipeline::new(config);

        // Conversation with no turn pairs (no assistant → tool/user sequence)
        let messages = vec![ConversationMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        }];

        let student_tokens: Vec<i32> = (0..10).collect();

        let result = pipeline
            .process_sequence(&messages, &student_tokens)
            .await
            .unwrap();

        // Should have no hints
        assert_eq!(result.num_turn_pairs, 0);
        assert_eq!(result.num_hints, 0);
        assert_eq!(result.avg_hint_confidence, 0.0);
    }

    #[tokio::test]
    async fn test_turn_pair_parser_integration() {
        let config = TurnPairParserConfig::default();
        let parser = TurnPairParser::new(config);

        let messages = vec![
            ConversationMessage {
                role: "user".to_string(),
                content: "Task 1".to_string(),
            },
            ConversationMessage {
                role: "assistant".to_string(),
                content: "Response 1".to_string(),
            },
            ConversationMessage {
                role: "tool".to_string(),
                content: "Result 1".to_string(),
            },
            ConversationMessage {
                role: "assistant".to_string(),
                content: "Response 2".to_string(),
            },
            ConversationMessage {
                role: "user".to_string(),
                content: "Feedback".to_string(),
            },
        ];

        let pairs = parser.extract_turn_pairs(&messages);

        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].assistant_text, "Response 1");
        assert_eq!(pairs[0].next_state_text, "Result 1");
        assert_eq!(pairs[0].next_state_role, "tool");
        assert_eq!(pairs[1].assistant_text, "Response 2");
        assert_eq!(pairs[1].next_state_text, "Feedback");
        assert_eq!(pairs[1].next_state_role, "user");
    }

    #[tokio::test]
    async fn test_multiple_tool_results_integration() {
        let config = TurnPairParserConfig::default();
        let parser = TurnPairParser::new(config);

        let messages = vec![
            ConversationMessage {
                role: "user".to_string(),
                content: "Run all tests".to_string(),
            },
            ConversationMessage {
                role: "assistant".to_string(),
                content: "Running tests...".to_string(),
            },
            ConversationMessage {
                role: "tool".to_string(),
                content: "Test 1: PASS".to_string(),
            },
            ConversationMessage {
                role: "tool".to_string(),
                content: "Test 2: FAIL - assertion error".to_string(),
            },
            ConversationMessage {
                role: "tool".to_string(),
                content: "Test 3: PASS".to_string(),
            },
        ];

        let pairs = parser.extract_turn_pairs(&messages);

        assert_eq!(pairs.len(), 1);
        let next_state = &pairs[0].next_state_text;
        assert!(next_state.contains("Test 1: PASS"));
        assert!(next_state.contains("Test 2: FAIL"));
        assert!(next_state.contains("Test 3: PASS"));
        assert!(next_state.contains("---")); // Separator between tool results
    }
}
