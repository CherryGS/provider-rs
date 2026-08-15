# provider-rs

`provider-rs` is a personal Rust workspace for the provider API endpoints used by
this project. Each endpoint is an independently callable capability with its own
request, response, credentials, and errors.

This is an interest-driven endpoint set, not a general-purpose or
production-ready SDK. Use official provider SDKs when broad API coverage or
stability guarantees matter.

## Capabilities

| Provider | Workspace crate / facade feature | Capabilities |
| --- | --- | --- |
| Anthropic | `provider-anthropic` / `anthropic` | Messages, token count, model list |
| Civitai | `provider-civitai` / `civitai` | Model search, model detail, preview image |
| Codex | `provider-codex` / `codex` | OAuth, account usage, model list, Responses |
| DeepSeek | `provider-deepseek` / `deepseek` | Chat Completions, Responses, model list, user balance |
| Exa | `provider-exa` / `exa` | Search |
| OpenAI | `provider-openai` / `openai` | Chat Completions, Responses, embeddings, model list |
| OpenCode | `provider-opencode` / `opencode` | Go quota usage |
| SiliconFlow | `provider-siliconflow` / `siliconflow` | Embeddings, rerank, model list |
| Volcengine | `provider-volcengine` / `volcengine` | Chat Completions, text and multimodal embeddings, Coding Plan usage, Agent Plan usage |

OpenCode Zen balance is intentionally absent because no observable balance API
endpoint currently exists; authenticated dashboard extraction is outside the
project scope.

## Usage

Capabilities accept a caller-owned `reqwest::Client` and explicit
provider-local credentials. For example, with the facade's `deepseek` feature
enabled:

```rust
use provider::deepseek::{Credentials, SecretString, capability::user_balance};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = SecretString::from(std::env::var("DEEPSEEK_API_KEY")?);
    let balance = user_balance::call(
        &reqwest::Client::new(),
        Credentials::new(&api_key),
    )
    .await?;

    for info in balance.balance_infos {
        println!("{} {}", info.currency, info.total_balance);
    }
    Ok(())
}
```

Optional composed clients exist only for providers where they remove useful
repetition. Standalone capability functions remain the primary API.

## CLI

The narrow CLI currently exposes Codex account usage:

```text
provider codex usage <auth path>
```

## Development

Run the complete formatting, lint, and test gate with:

```text
just rust-finalize
```

See [`project-doc/INTENT.md`](project-doc/INTENT.md) for project scope and
[`project-doc/design/standard/`](project-doc/design/standard/) for settled design
contracts.
