use provider::codex::{Credentials, ExposeSecret, SecretString, account_usage::call};
use serde::Deserialize;
use std::{env, fs, io, io::Write, process::ExitCode};

const USAGE: &str = "usage: provider codex usage <auth path>";

#[derive(Deserialize)]
struct AuthFile {
    tokens: Option<Tokens>,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: SecretString,
    account_id: Option<SecretString>,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let (Some(provider), Some(capability), Some(auth_path), None) =
        (args.next(), args.next(), args.next(), args.next())
    else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, USAGE).into());
    };
    if provider != "codex" || capability != "usage" {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, USAGE).into());
    }

    let auth: AuthFile = serde_json::from_slice(&fs::read(auth_path)?)?;
    let tokens = auth.tokens.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Codex auth JSON must contain ChatGPT tokens",
        )
    })?;
    if tokens.access_token.expose_secret().trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Codex auth tokens must contain an access token",
        )
        .into());
    }
    let account_id = tokens
        .account_id
        .as_ref()
        .filter(|account_id| !account_id.expose_secret().trim().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Codex auth tokens must contain an account ID",
            )
        })?;

    let usage = call(
        &reqwest::Client::new(),
        Credentials {
            access_token: &tokens.access_token,
            account_id,
        },
    )
    .await?;

    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer(&mut stdout, &usage)?;
    stdout.write_all(b"\n")?;
    Ok(())
}
