// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Credential lifecycle commands backed by the canonical HTTP endpoints.

use std::{
    io::{self, Read},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Args as ClapArgs, Subcommand};
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use zeta4j_cli_grpc::credential::{
    CredentialEnvelope, default_credential_path, load_credential_envelope,
    refresh_credential_envelope, store_credential_envelope,
};
use zeta4j_transport_security::client_tls_config_from_environment;

#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Credential lifecycle HTTP endpoint.
    #[arg(long, default_value = "http://127.0.0.1:9044")]
    endpoint: String,
    /// Owner-only credential envelope path.
    #[arg(long)]
    credential_file: Option<PathBuf>,
    /// Exact protocol audience stored in the envelope.
    #[arg(long, default_value = "grpc", value_parser = ["grpc", "bolt"])]
    audience: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Authenticate and save an owner-only gRPC credential envelope.
    Login {
        #[arg(long)]
        username: String,
        /// Read the password from standard input instead of a terminal prompt.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Atomically consume the stored refresh credential.
    Refresh,
    /// Close the current session and remove the local envelope.
    Logout,
    /// Show the immutable principal represented by the stored credential.
    Whoami,
    /// List this principal's durable sessions.
    Sessions,
    /// Revoke one durable session owned by this principal.
    RevokeSession {
        #[arg(long)]
        session_id: String,
    },
}

#[derive(Serialize)]
struct LoginRequest<'a> {
    username: &'a str,
    password: &'a str,
    audience: &'static str,
}

#[derive(Serialize)]
struct RevokeSessionRequest<'a> {
    session_id: &'a str,
    audience: &'static str,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    access_expires_at: i64,
    refresh_expires_at: i64,
    session_id: String,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: String,
    message: String,
}

pub fn execute(args: Args) -> ExitCode {
    match execute_inner(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute_inner(args: Args) -> Result<(), String> {
    let endpoint = normalized_endpoint(&args.endpoint)?;
    let path = match args.credential_file {
        Some(path) => path,
        None if args.audience == "bolt" => default_bolt_credential_path()?,
        None => default_credential_path().map_err(|error| error.to_string())?,
    };
    let client = credential_client(&endpoint)?;
    match args.command {
        Command::Login {
            username,
            password_stdin,
        } => {
            let password = read_password(password_stdin)?;
            let response = checked(
                client
                    .post(format!("{endpoint}/auth/login"))
                    .json(&LoginRequest {
                        username: &username,
                        password: &password,
                        audience: audience_static(&args.audience)?,
                    })
                    .send(),
            )?
            .json::<TokenResponse>()
            .map_err(|_| "credential response was malformed".to_string())?;
            save_response(&path, &endpoint, &args.audience, response)?;
            println!("Authenticated credential saved to {}", path.display());
        }
        Command::Refresh => {
            refresh_credential_envelope(&path, unix_now()?).map_err(|error| error.to_string())?;
            println!("Credential refreshed");
        }
        Command::Logout => {
            let envelope = load_credential_envelope(&path).map_err(|error| error.to_string())?;
            checked(
                client
                    .post(format!(
                        "{endpoint}/auth/logout?audience={}",
                        envelope.audience
                    ))
                    .bearer_auth(envelope.access_token())
                    .send(),
            )?;
            std::fs::remove_file(&path)
                .map_err(|_| "failed to remove credential envelope".to_string())?;
            println!("Session closed");
        }
        Command::Whoami => {
            let response = authenticated_get(&client, &endpoint, &path, "/auth/whoami")?;
            print_json(response)?;
        }
        Command::Sessions => {
            let response = authenticated_get(&client, &endpoint, &path, "/auth/sessions")?;
            print_json(response)?;
        }
        Command::RevokeSession { session_id } => {
            let envelope = load_credential_envelope(&path).map_err(|error| error.to_string())?;
            checked(
                client
                    .post(format!("{endpoint}/auth/revoke-session"))
                    .bearer_auth(envelope.access_token())
                    .json(&RevokeSessionRequest {
                        session_id: &session_id,
                        audience: audience_static(&envelope.audience)?,
                    })
                    .send(),
            )?;
            println!("Session revoked");
        }
    }
    Ok(())
}

fn authenticated_get(
    client: &Client,
    endpoint: &str,
    path: &Path,
    route: &str,
) -> Result<Response, String> {
    let envelope = load_credential_envelope(path).map_err(|error| error.to_string())?;
    checked(
        client
            .get(format!("{endpoint}{route}?audience={}", envelope.audience))
            .bearer_auth(envelope.access_token())
            .send(),
    )
}

fn save_response(
    path: &Path,
    endpoint: &str,
    audience: &str,
    response: TokenResponse,
) -> Result<(), String> {
    let envelope = CredentialEnvelope::new(
        audience,
        endpoint,
        response.access_token,
        response.refresh_token,
        response.access_expires_at,
        response.refresh_expires_at,
        response.session_id,
    )
    .map_err(|error| error.to_string())?;
    store_credential_envelope(path, &envelope).map_err(|error| error.to_string())
}

fn audience_static(audience: &str) -> Result<&'static str, String> {
    match audience {
        "grpc" => Ok("grpc"),
        "bolt" => Ok("bolt"),
        _ => Err("unsupported credential audience".to_string()),
    }
}

fn checked(response: Result<Response, reqwest::Error>) -> Result<Response, String> {
    let response = response.map_err(|_| "credential service unavailable".to_string())?;
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let failure = response.json::<ErrorResponse>().ok();
    Err(failure.map_or_else(
        || format!("credential request failed with HTTP {status}"),
        |failure| format!("[{}] {}", failure.error, failure.message),
    ))
}

fn print_json(response: Response) -> Result<(), String> {
    let value = response
        .json::<serde_json::Value>()
        .map_err(|_| "credential response was malformed".to_string())?;
    let output = serde_json::to_string_pretty(&value)
        .map_err(|_| "credential response was malformed".to_string())?;
    println!("{output}");
    Ok(())
}

fn normalized_endpoint(endpoint: &str) -> Result<String, String> {
    let endpoint = endpoint.trim_end_matches('/');
    let parsed = reqwest::Url::parse(endpoint)
        .map_err(|_| "credential endpoint is malformed".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("credential endpoint must use http:// or https://".to_string());
    }
    if parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !matches!(parsed.path(), "" | "/")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("credential endpoint must be an exact HTTP origin".to_string());
    }
    Ok(endpoint.to_string())
}

fn credential_client(endpoint: &str) -> Result<Client, String> {
    let parsed = reqwest::Url::parse(endpoint)
        .map_err(|_| "credential endpoint is malformed".to_string())?;
    match parsed.scheme() {
        "http" => Client::builder()
            .build()
            .map_err(|_| "credential HTTP client initialization failed".to_string()),
        "https" => {
            let host = parsed
                .host_str()
                .ok_or_else(|| "credential endpoint has no host".to_string())?;
            let (config, _) = client_tls_config_from_environment(&[b"http/1.1"], host)
                .map_err(|error| error.to_string())?;
            Client::builder()
                .https_only(true)
                .use_preconfigured_tls(config)
                .build()
                .map_err(|_| "credential HTTPS client initialization failed".to_string())
        }
        _ => Err("credential endpoint must use http or https".to_string()),
    }
}

fn read_password(from_stdin: bool) -> Result<String, String> {
    if from_stdin {
        let mut password = String::new();
        io::stdin()
            .read_to_string(&mut password)
            .map_err(|_| "failed to read password from standard input".to_string())?;
        while password.ends_with(['\n', '\r']) {
            password.pop();
        }
        return Ok(password);
    }
    rpassword::prompt_password("Password: ")
        .map_err(|_| "failed to read password from terminal".to_string())
}

fn unix_now() -> Result<i64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .ok_or_else(|| "system clock is outside the supported range".to_string())
}

fn default_bolt_credential_path() -> Result<PathBuf, String> {
    if let Some(home) = std::env::var_os("ZETA4J_HOME") {
        return Ok(PathBuf::from(home).join("credentials").join("bolt.json"));
    }
    dirs::home_dir()
        .map(|home| home.join(".zeta4j").join("credentials").join("bolt.json"))
        .ok_or_else(|| "credential home directory is unavailable".to_string())
}

#[cfg(test)]
mod tests {
    use super::credential_client;

    #[test]
    fn endpoint_scheme_selects_the_transport() {
        assert!(credential_client("http://192.0.2.10:9044").is_ok());
        assert!(credential_client("ftp://192.0.2.10:9044").is_err());
    }
}
