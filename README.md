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
| TypeSafe (Jev) | `provider-typesafe` / `typesafe` | System One (Choice, Score, Noul), model list |
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

For Jev, enable the facade's `typesafe` feature (or depend on `provider-typesafe`
directly). The caller loads `TYPESAFE_API_KEY` and passes it explicitly:

```rust
use provider::typesafe::{Credentials, SecretString, capability::system_one};

async fn evaluate() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = SecretString::from(std::env::var("TYPESAFE_API_KEY")?);
    let request = system_one::Request::new(
        "jev-latest",
        "The nightly export failed and the report is due this morning.",
        [("urgent".into(), system_one::Question::noul("Does this need urgent attention?"))].into(),
    );
    let response = system_one::call(
        &reqwest::Client::new(),
        Credentials::new(&api_key),
        &request,
    ).await?;
    if let Some(system_one::Answer::Noul(answer)) = response.answers.get("urgent") {
        println!("Urgency probability: {}", answer.noul);
    }
    Ok(())
}
```

`Question::choice` and `Question::score` can be mixed with Noul questions in the
same request. Responses preserve probability distributions, confidence, score
legends, and optional token counts. `typesafe::capability::model_list::call`
lists available models; `typesafe::Client` optionally binds credentials and the
HTTP client for both endpoints. Contracts follow the official
[System One API](https://docs.typesafe.ai/api) and
[model list](https://docs.typesafe.ai/models).

Errors distinguish request/exchange failures from `BodyRead { status, source }`
failures after HTTP headers arrive. `status()` preserves the received status,
including a successful status when its body is truncated, and
`std::error::Error::source()` preserves the underlying reqwest error. Consumers
that exhaustively match an endpoint's error enum must handle `BodyRead`.

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
