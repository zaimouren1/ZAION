# Provider Setup

Zaion currently treats these as stable provider paths:

- Anthropic
- OpenAI
- Groq
- Mistral
- Ollama

Phase 7's expansion order promotes Ollama, OpenAI, and Anthropic first. Groq and
Mistral remain stable compatible providers because the shared provider
resolution, doctor, chat, wake, Telegram, and TUI paths already cover them.

Run `zaion doctor` after every provider change.

## Anthropic

```bash
zaion config set provider anthropic
zaion config set anthropic_api_key <key>
zaion config set model claude-sonnet-4-6
zaion doctor
```

Environment alternative: `ANTHROPIC_API_KEY`.

## OpenAI

```bash
zaion config set provider openai
zaion config set openai_api_key <key>
zaion config set openai_base_url https://api.openai.com/v1
zaion config set model gpt-4o
zaion doctor
```

Environment alternative: `OPENAI_API_KEY`. Override `OPENAI_BASE_URL` for an
OpenAI-compatible endpoint.

## Groq

```bash
zaion config set provider groq
zaion config set groq_api_key <key>
zaion config set groq_base_url https://api.groq.com/openai/v1
zaion config set model llama-3.3-70b-versatile
zaion doctor
```

Environment alternative: `GROQ_API_KEY`.

## Mistral

```bash
zaion config set provider mistral
zaion config set mistral_api_key <key>
zaion config set mistral_base_url https://api.mistral.ai/v1
zaion config set model mistral-large-latest
zaion doctor
```

Environment alternative: `MISTRAL_API_KEY`.

## Ollama

```bash
ollama pull llama3.2
zaion config set provider ollama
zaion config set ollama_base_url http://localhost:11434/v1
zaion config set model llama3.2
zaion doctor
zaion chat "Hello"
```

Ollama does not require an API key. Keep the base URL on the OpenAI-compatible
`/v1` endpoint.
