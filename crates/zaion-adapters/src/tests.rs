use crate::{
    channel::{ChannelAdapter, ChannelType, OutboundMessage, TelegramAdapter, TerminalAdapter},
    provider::{AnthropicProvider, LlmProvider, OpenAiProvider, ProviderType},
};
use zaion_types::session::ChannelId;

#[test]
fn test_terminal_adapter_send() {
    let adapter = TerminalAdapter::new(ChannelId("terminal".into()));
    assert_eq!(adapter.channel_type(), ChannelType::Terminal);
    let msg = OutboundMessage {
        channel_id: "terminal".into(),
        thread_id: "t1".into(),
        text: "hello zaion".into(),
        reply_to: None,
        metadata: serde_json::json!({}),
        parse_mode: None,
    };
    assert!(adapter.send(&msg).is_ok());
    let received = adapter.receive().unwrap();
    assert_eq!(received.len(), 0);
}

#[test]
fn test_telegram_adapter_no_token_fails() {
    let adapter = TelegramAdapter::new("", ChannelId("telegram".into()));
    assert_eq!(adapter.channel_type(), ChannelType::Telegram);
    let msg = OutboundMessage {
        channel_id: "telegram".into(),
        thread_id: "t1".into(),
        text: "test".into(),
        reply_to: None,
        metadata: serde_json::json!({}),
        parse_mode: None,
    };
    assert!(adapter.send(&msg).is_err());
}

#[test]
fn test_openai_provider_stub() {
    let provider = OpenAiProvider::new("https://api.openai.com", "sk-test", "gpt-4o");
    assert_eq!(provider.provider_type(), ProviderType::OpenAiCompatible);
}

#[test]
fn test_anthropic_provider_stub() {
    let provider = AnthropicProvider::new("sk-ant-test", "claude-sonnet-4-6");
    assert_eq!(provider.provider_type(), ProviderType::Anthropic);
}
