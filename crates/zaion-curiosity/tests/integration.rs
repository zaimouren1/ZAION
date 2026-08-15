//! Integration tests for System V: Entropic Curiosity
//!
//! Tests idle detection, spontaneous ideation, and LLM-driven exploration.

use std::thread;
use std::time::Duration;
use zaion_curiosity::{IdeationCategory, IdeationConfig, IdeationLoop, IdleState, IdleTimer};

#[test]
fn test_idle_timer_initialization() {
    let timer = IdleTimer::new(Duration::from_secs(60));
    assert_eq!(timer.state(), IdleState::Active);
    assert!(!timer.is_idle());
    assert!(!timer.is_deep_idle());
}

#[test]
fn test_idle_timer_with_custom_thresholds() {
    let timer = IdleTimer::with_thresholds(Duration::from_secs(30), Duration::from_secs(120));

    assert_eq!(timer.state(), IdleState::Active);
    assert_eq!(timer.time_since_activity().as_secs(), 0);
}

#[test]
fn test_idle_timer_becomes_idle() {
    let timer = IdleTimer::new(Duration::from_millis(50));
    thread::sleep(Duration::from_millis(60));

    assert_eq!(timer.state(), IdleState::Idle);
    assert!(timer.is_idle());
    assert!(!timer.is_deep_idle());
}

#[test]
fn test_idle_timer_becomes_deep_idle() {
    let timer = IdleTimer::new(Duration::from_millis(20));
    thread::sleep(Duration::from_millis(70));

    assert_eq!(timer.state(), IdleState::DeepIdle);
    assert!(timer.is_idle());
    assert!(timer.is_deep_idle());
}

#[test]
fn test_idle_timer_reset() {
    let mut timer = IdleTimer::new(Duration::from_millis(50));
    thread::sleep(Duration::from_millis(60));
    assert!(timer.is_idle());

    timer.reset();
    assert_eq!(timer.state(), IdleState::Active);
    assert!(!timer.is_idle());
}

#[test]
fn test_idle_timer_percentage() {
    let timer = IdleTimer::new(Duration::from_secs(1));
    assert_eq!(timer.idle_percentage(), 0.0);

    // Percentage only starts after threshold
    let timer = IdleTimer::new(Duration::from_millis(10));
    thread::sleep(Duration::from_millis(15));
    assert!(timer.idle_percentage() > 0.0);
}

#[test]
fn test_ideation_category_all() {
    let categories = IdeationCategory::all();
    assert_eq!(categories.len(), 6);
    assert!(categories.contains(&IdeationCategory::Exploration));
    assert!(categories.contains(&IdeationCategory::Optimization));
    assert!(categories.contains(&IdeationCategory::Refactoring));
    assert!(categories.contains(&IdeationCategory::Documentation));
    assert!(categories.contains(&IdeationCategory::Testing));
    assert!(categories.contains(&IdeationCategory::Security));
}

#[test]
fn test_ideation_category_random() {
    let cat1 = IdeationCategory::random();
    let cat2 = IdeationCategory::random();

    // Verify they're valid categories
    assert!(IdeationCategory::all().contains(&cat1));
    assert!(IdeationCategory::all().contains(&cat2));
}

#[test]
fn test_ideation_config_defaults() {
    let config = IdeationConfig::default();
    assert!(config.enabled);
    assert_eq!(config.min_idle_seconds, 300);
    assert_eq!(config.categories.len(), 6);
}

#[test]
fn test_ideation_config_custom() {
    let config = IdeationConfig {
        enabled: false,
        min_idle_seconds: 600,
        categories: vec![IdeationCategory::Exploration, IdeationCategory::Security],
    };

    assert!(!config.enabled);
    assert_eq!(config.min_idle_seconds, 600);
    assert_eq!(config.categories.len(), 2);
}

#[test]
fn test_ideation_loop_initialization() {
    let loop_instance = IdeationLoop::default();
    // Loop starts with no last ideation
    assert!(loop_instance.should_ideate(400));
}

#[test]
fn test_ideation_loop_disabled() {
    let config = IdeationConfig {
        enabled: false,
        ..IdeationConfig::default()
    };

    let loop_instance = IdeationLoop::new(config);
    assert!(!loop_instance.should_ideate(1000));
}

#[test]
fn test_ideation_loop_below_threshold() {
    let loop_instance = IdeationLoop::default();
    assert!(!loop_instance.should_ideate(100));
    assert!(!loop_instance.should_ideate(299));
}

#[test]
fn test_ideation_loop_above_threshold() {
    let loop_instance = IdeationLoop::default();
    assert!(loop_instance.should_ideate(300));
    assert!(loop_instance.should_ideate(500));
}

#[test]
fn test_ideation_loop_generate_prompt() {
    let mut loop_instance = IdeationLoop::default();
    let prompt = loop_instance.generate_prompt();

    assert!(prompt.is_some());
    let prompt = prompt.unwrap();
    assert!(!prompt.prompt.is_empty());
    assert!(IdeationCategory::all().contains(&prompt.category));
}

#[test]
fn test_ideation_loop_respects_cooldown() {
    let mut loop_instance = IdeationLoop::default();

    // First ideation should work
    assert!(loop_instance.generate_prompt().is_some());

    // Immediately after, should not ideate again
    assert!(!loop_instance.should_ideate(400));
}

#[test]
fn test_ideation_loop_reset() {
    let mut loop_instance = IdeationLoop::default();
    loop_instance.generate_prompt();

    // After reset, should be able to ideate again
    loop_instance.reset();
    assert!(loop_instance.should_ideate(400));
}

#[test]
fn test_ideation_prompt_structure() {
    let mut loop_instance = IdeationLoop::default();
    let prompt = loop_instance.generate_prompt().unwrap();

    // Verify prompt structure
    assert!(!prompt.prompt.is_empty());
    assert!(matches!(
        prompt.category,
        IdeationCategory::Exploration
            | IdeationCategory::Optimization
            | IdeationCategory::Refactoring
            | IdeationCategory::Documentation
            | IdeationCategory::Testing
            | IdeationCategory::Security
    ));

    // Verify timestamp is recent
    let now = chrono::Utc::now();
    let diff = (now - prompt.generated_at).num_seconds();
    assert!(diff < 5);
}

#[test]
fn test_end_to_end_curiosity_workflow() {
    // 1. Create idle timer
    let mut timer = IdleTimer::new(Duration::from_millis(100));
    assert_eq!(timer.state(), IdleState::Active);

    // 2. Wait for idle state
    thread::sleep(Duration::from_millis(120));
    assert!(timer.is_idle());

    // 3. Create ideation loop
    let config = IdeationConfig {
        enabled: true,
        min_idle_seconds: 0, // Allow immediate ideation for testing
        categories: IdeationCategory::all(),
    };
    let mut ideation = IdeationLoop::new(config);

    // 4. Check if should ideate
    let idle_seconds = timer.time_since_activity().as_secs();
    assert!(ideation.should_ideate(idle_seconds));

    // 5. Generate prompt
    let prompt = ideation.generate_prompt();
    assert!(prompt.is_some());
    let prompt = prompt.unwrap();
    assert!(!prompt.prompt.is_empty());

    // 6. Reset timer on activity
    timer.reset();
    assert_eq!(timer.state(), IdleState::Active);
    assert!(!timer.is_idle());
}

#[test]
fn test_llm_ideation_codebase_context() {
    use zaion_curiosity::{build_system_prompt, CodebaseContext};

    let ctx = CodebaseContext {
        recent_diff_summary: "Modified auth.rs and db.rs".to_string(),
        indexed_files: vec![
            "src/main.rs".to_string(),
            "src/auth.rs".to_string(),
            "src/db.rs".to_string(),
        ],
        ast_chunk_count: 42,
        category: IdeationCategory::Security,
    };

    let system_prompt = build_system_prompt(&ctx);

    // Verify system prompt contains context
    assert!(system_prompt.contains("42"));
    assert!(system_prompt.contains("auth.rs"));
    assert!(system_prompt.contains("security"));
}

#[test]
fn test_llm_ideation_empty_context() {
    use zaion_curiosity::{build_system_prompt, CodebaseContext};

    let ctx = CodebaseContext {
        recent_diff_summary: String::new(),
        indexed_files: vec![],
        ast_chunk_count: 0,
        category: IdeationCategory::Exploration,
    };

    let system_prompt = build_system_prompt(&ctx);

    // Should handle empty context gracefully
    assert!(system_prompt.contains("explore"));
    assert!(system_prompt.contains("none indexed yet"));
}

#[test]
fn test_ideation_category_specific_prompts() {
    let categories = vec![
        IdeationCategory::Exploration,
        IdeationCategory::Optimization,
        IdeationCategory::Refactoring,
        IdeationCategory::Documentation,
        IdeationCategory::Testing,
        IdeationCategory::Security,
    ];

    for category in categories {
        let config = IdeationConfig {
            enabled: true,
            min_idle_seconds: 0,
            categories: vec![category],
        };

        let mut loop_instance = IdeationLoop::new(config);
        let prompt = loop_instance.generate_prompt();

        assert!(prompt.is_some());
        let prompt = prompt.unwrap();
        assert_eq!(prompt.category, category);
        assert!(!prompt.prompt.is_empty());
    }
}
