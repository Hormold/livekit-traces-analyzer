//! LiveKit Cloud API client (experimental).
//!
//! Reads credentials from `~/.livekit/cli-config.yaml` (shared with `lk` CLI),
//! lists projects/sessions, and downloads observability data.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

const CLOUD_API_BASE: &str = "https://cloud-api.livekit.io";
const SESSION_TOKEN_FILE: &str = "session-token";

// ---------------------------------------------------------------------------
// CLI config reader (~/.livekit/cli-config.yaml)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CliConfig {
    pub default_project: Option<String>,
    #[serde(default)]
    pub projects: Vec<ProjectConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProjectConfig {
    pub name: String,
    pub project_id: Option<String>,
    pub url: String,
    pub api_key: String,
    pub api_secret: String,
}

pub fn load_config() -> Result<CliConfig> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let path = PathBuf::from(home).join(".livekit").join("cli-config.yaml");

    if !path.exists() {
        bail!(
            "LiveKit CLI config not found at {}.\n\
             Run `lk cloud auth` first to authenticate with LiveKit Cloud.",
            path.display()
        );
    }

    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let config: CliConfig = serde_yaml::from_str(&contents)
        .with_context(|| format!("Failed to parse {}", path.display()))?;

    if config.projects.is_empty() {
        bail!(
            "No projects in {}.\nRun `lk cloud auth` to add a project.",
            path.display()
        );
    }

    Ok(config)
}

pub fn resolve_project<'a>(config: &'a CliConfig, name: Option<&str>) -> Result<&'a ProjectConfig> {
    match name {
        Some(n) => config
            .projects
            .iter()
            .find(|p| p.name == n)
            .with_context(|| {
                let available: Vec<&str> = config.projects.iter().map(|p| p.name.as_str()).collect();
                format!(
                    "Project '{}' not found. Available: {}",
                    n,
                    available.join(", ")
                )
            }),
        None => {
            let default_name = config.default_project.as_deref().with_context(|| {
                "No --project specified and no default_project in config"
            })?;
            config
                .projects
                .iter()
                .find(|p| p.name == default_name)
                .with_context(|| format!("Default project '{}' not found in config", default_name))
        }
    }
}

// ---------------------------------------------------------------------------
// JWT token generation
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct VideoGrant {
    #[serde(rename = "roomList")]
    room_list: bool,
}

#[derive(Debug, Serialize)]
struct TokenClaims {
    iss: String,
    sub: String,
    exp: u64,
    nbf: u64,
    video: VideoGrant,
}

pub fn generate_token(project: &ProjectConfig) -> Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("Clock error")?
        .as_secs();

    let claims = TokenClaims {
        iss: project.api_key.clone(),
        sub: "livekit-analyzer".to_string(),
        exp: now + 3600, // 1 hour
        nbf: now,
        video: VideoGrant { room_list: true },
    };

    let key = jsonwebtoken::EncodingKey::from_secret(project.api_secret.as_bytes());
    let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);

    jsonwebtoken::encode(&header, &claims, &key).context("Failed to generate JWT token")
}

// ---------------------------------------------------------------------------
// REST API client
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SessionsResponse {
    #[serde(default)]
    pub sessions: Vec<SessionSummary>,
}

/// The LiveKit REST API inconsistently returns numbers as strings or ints.
/// Use `serde_json::Value` + accessor for all potentially-numeric fields.
#[derive(Debug, Deserialize, Serialize)]
pub struct SessionSummary {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "roomName", default)]
    pub room_name: String,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
    #[serde(rename = "endedAt", default)]
    pub ended_at: String,
    #[serde(rename = "lastActive", default)]
    pub last_active: String,
    #[serde(rename = "numParticipants", default)]
    pub num_participants: serde_json::Value,
    #[serde(rename = "numActiveParticipants", default)]
    pub num_active_participants: serde_json::Value,
    #[serde(rename = "bandwidthIn", default)]
    pub bandwidth_in: serde_json::Value,
    #[serde(rename = "bandwidthOut", default)]
    pub bandwidth_out: serde_json::Value,
    // Catch any other fields without failing
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl SessionSummary {
    pub fn participants(&self) -> u64 {
        json_to_u64(&self.num_participants)
    }
}

fn json_to_u64(v: &serde_json::Value) -> u64 {
    match v {
        serde_json::Value::Number(n) => n.as_u64().unwrap_or(0),
        serde_json::Value::String(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SessionDetail {
    #[serde(rename = "roomId", default)]
    pub room_id: String,
    #[serde(rename = "roomName", default)]
    pub room_name: String,
    #[serde(rename = "startTime", default)]
    pub start_time: String,
    #[serde(rename = "endTime", default)]
    pub end_time: String,
    #[serde(rename = "numParticipants", default)]
    pub num_participants: serde_json::Value,
    #[serde(rename = "connectionMinutes", default)]
    pub connection_minutes: serde_json::Value,
    #[serde(default)]
    pub participants: Vec<Participant>,
    #[serde(default)]
    pub bandwidth: serde_json::Value,
    // Catch any other fields
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Participant {
    #[serde(rename = "participantIdentity", default)]
    pub identity: String,
    #[serde(rename = "participantName", default)]
    pub name: String,
    #[serde(rename = "joinedAt", default)]
    pub joined_at: String,
    #[serde(rename = "leftAt", default)]
    pub left_at: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub region: String,
    #[serde(rename = "connectionType", default)]
    pub connection_type: String,
    #[serde(rename = "deviceModel", default)]
    pub device_model: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub browser: String,
    #[serde(rename = "sdkVersion", default)]
    pub sdk_version: String,
}

fn api_get<T: serde::de::DeserializeOwned>(token: &str, path: &str) -> Result<T> {
    let url = format!("{}{}", CLOUD_API_BASE, path);
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .send()
        .with_context(|| format!("Request failed: GET {}", url))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        bail!("API error {} for GET {}: {}", status, url, body);
    }

    resp.json::<T>()
        .with_context(|| format!("Failed to parse response from GET {}", url))
}

pub fn list_sessions(
    token: &str,
    project_id: &str,
    limit: u32,
    page: u32,
) -> Result<SessionsResponse> {
    let path = format!(
        "/api/project/{}/sessions?limit={}&page={}",
        project_id, limit, page
    );
    api_get(token, &path)
}

pub fn get_session_detail(
    token: &str,
    project_id: &str,
    session_id: &str,
) -> Result<SessionDetail> {
    let path = format!("/api/project/{}/sessions/{}", project_id, session_id);
    api_get(token, &path)
}

// ---------------------------------------------------------------------------
// Session token management (saved to ~/.livekit/session-token)
// ---------------------------------------------------------------------------

fn token_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".livekit").join(SESSION_TOKEN_FILE)
}

/// Load saved session token from disk.
fn load_session_token() -> Option<String> {
    let path = token_path();
    std::fs::read_to_string(&path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Save session token to disk.
fn save_session_token(token: &str) -> Result<()> {
    let path = token_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, token)?;
    // Restrict permissions (owner-only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Prompt the user interactively to paste their session token.
fn prompt_session_token() -> Result<String> {
    eprintln!();
    eprintln!("Session token required for downloading observability data.");
    eprintln!();
    eprintln!("To get it:");
    eprintln!("  1. Open https://cloud.livekit.io and log in");
    eprintln!("  2. Open DevTools (F12) → Application → Cookies → cloud.livekit.io");
    eprintln!("  3. Find `__Secure-authjs.browser-session-token`");
    eprintln!("  4. Double-click the Value column and copy it");
    eprintln!();
    eprint!("Paste token: ");
    std::io::stderr().flush()?;

    let mut token = String::new();
    std::io::stdin().read_line(&mut token).context("Failed to read token from stdin")?;
    let token = token.trim().to_string();

    if token.is_empty() {
        bail!("No token provided");
    }

    Ok(token)
}

/// Resolve session token: --token flag > LK_CLOUD_TOKEN env > saved file > interactive prompt.
/// Saves new tokens for reuse.
fn resolve_session_token(flag_token: Option<&str>) -> Result<String> {
    // 1. Explicit --token flag (always save it)
    if let Some(t) = flag_token {
        save_session_token(t).ok();
        return Ok(t.to_string());
    }

    // 2. LK_CLOUD_TOKEN env var
    if let Ok(t) = std::env::var("LK_CLOUD_TOKEN") {
        let t = t.trim().to_string();
        if !t.is_empty() {
            save_session_token(&t).ok();
            return Ok(t);
        }
    }

    // 3. Saved token file
    if let Some(t) = load_session_token() {
        return Ok(t);
    }

    // 4. Interactive prompt
    let t = prompt_session_token()?;
    save_session_token(&t)?;
    eprintln!("Token saved to {}", token_path().display());
    Ok(t)
}

/// Delete the saved session token (called when download gets 401).
fn clear_session_token() {
    let path = token_path();
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// OTLP download via REST ZIP endpoint
// ---------------------------------------------------------------------------

/// Download OTLP observability data as a ZIP file and extract it.
///
/// The cloud API serves a ZIP at:
///   GET /api/project/{project_id}/sessions/{session_id}/otlp.zip
///
/// This requires a **session token** (from browser login), NOT a JWT.
/// Pass the token via `--token` flag. To get it:
///   1. Log in to cloud.livekit.io
///   2. Open DevTools → Application → Cookies
///   3. Copy the value of `__Secure-authjs.browser-session-token`
fn try_download_otlp_zip(
    session_token: &str,
    project_id: &str,
    session_id: &str,
    output_dir: &PathBuf,
) -> Result<bool> {
    let url = format!(
        "{}/api/project/{}/sessions/{}/otlp.zip",
        CLOUD_API_BASE, project_id, session_id
    );

    let client = reqwest::blocking::Client::builder()
        .user_agent("livekit-analyzer/0.4")
        .build()?;

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", session_token))
        .send()
        .with_context(|| format!("Failed to request {}", url))?;

    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        let body = resp.text().unwrap_or_default();
        if body.contains("not logged in") || body.contains("1010") || status.as_u16() == 401 {
            return Ok(false); // Token expired/invalid
        }
        bail!("Download failed: {} {}", status, body);
    }
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        bail!("Download failed: {} {}", status, body);
    }

    let zip_bytes = resp.bytes().context("Failed to read ZIP response")?;
    eprintln!("      Downloaded {} bytes", zip_bytes.len());

    // Extract ZIP into output directory
    let cursor = std::io::Cursor::new(&zip_bytes[..]);
    let mut archive = zip::ZipArchive::new(cursor)
        .context("Failed to parse ZIP archive")?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();

        // The ZIP contains files like: p_xxx_RM_xxx_traces.json
        // Rename to standard names the analyzer expects:
        //   *_traces.json → spans.json
        //   *_logs.json   → logs.json
        //   *_audio.*     → audio.oga
        //   *_chat_history.json → chat_history.json
        let target_name = if name.ends_with("_traces.json") {
            "spans.json".to_string()
        } else if name.ends_with("_logs.json") {
            "logs.json".to_string()
        } else if name.contains("_audio.") {
            let ext = name.rsplit('.').next().unwrap_or("oga");
            format!("audio.{}", ext)
        } else if name.ends_with("_chat_history.json") {
            "chat_history.json".to_string()
        } else {
            name.clone()
        };

        let out_path = output_dir.join(&target_name);

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut out_file = std::fs::File::create(&out_path)?;
        std::io::copy(&mut file, &mut out_file)?;
        eprintln!("      {} → {}", name, target_name);
    }

    Ok(true)
}

/// Download OTLP ZIP with automatic token retry.
/// If the token is expired, clears saved token and prompts for a new one.
fn download_otlp_zip(
    session_token: &str,
    project_id: &str,
    session_id: &str,
    output_dir: &PathBuf,
) -> Result<()> {
    eprintln!("[2/2] Downloading OTLP data...");

    // First attempt with current token
    if try_download_otlp_zip(session_token, project_id, session_id, output_dir)? {
        return Ok(());
    }

    // Token expired — clear saved token and re-prompt
    eprintln!("Session token expired or invalid.");
    clear_session_token();

    let new_token = prompt_session_token()?;
    save_session_token(&new_token)?;
    eprintln!("Token saved to {}", token_path().display());
    eprintln!("Retrying download...");

    if try_download_otlp_zip(&new_token, project_id, session_id, output_dir)? {
        return Ok(());
    }

    bail!(
        "Authentication failed. Make sure you copied the correct cookie value.\n\
         Cookie name: __Secure-authjs.browser-session-token"
    );
}

pub fn download_session(
    jwt_token: &str,
    session_token: &str,
    project_id: &str,
    session_id: &str,
    output_dir: &PathBuf,
) -> Result<()> {
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output dir: {}", output_dir.display()))?;

    // Save session metadata (uses JWT — always works)
    eprintln!("[1/2] Fetching session metadata...");
    let detail = get_session_detail(jwt_token, project_id, session_id)?;
    let metadata_path = output_dir.join("metadata.json");
    let metadata_json = serde_json::to_string_pretty(&detail)?;
    std::fs::write(&metadata_path, &metadata_json)?;
    eprintln!("      Saved {}", metadata_path.display());

    // Download OTLP ZIP (requires session token)
    download_otlp_zip(session_token, project_id, session_id, output_dir)?;

    eprintln!();
    eprintln!("Done! Output: {}", output_dir.display());
    eprintln!("Analyze with: livekit-analyzer {} --dump", output_dir.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// CLI subcommand handler
// ---------------------------------------------------------------------------

pub struct CloudOptions {
    pub command: CloudCommand,
    pub project_name: Option<String>,
    pub session_token: Option<String>,
}

pub enum CloudCommand {
    Projects,
    Sessions { limit: u32, page: u32, json: bool },
    Info { session_id: String },
    Download { session_id: String, output: Option<PathBuf> },
}

pub fn parse_cloud_args(args: &[String]) -> Result<CloudOptions, String> {
    if args.is_empty() {
        return Err("Missing cloud subcommand. Use: projects, sessions, info, download".to_string());
    }

    let mut project_name: Option<String> = None;
    let mut session_token: Option<String> = None;
    let mut limit: u32 = 20;
    let mut page: u32 = 0;
    let mut json_output = false;
    let mut output: Option<PathBuf> = None;
    let mut positional: Vec<String> = Vec::new();

    let subcmd = &args[0];
    let rest = &args[1..];

    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--project" | "-p" => {
                i += 1;
                if i >= rest.len() {
                    return Err("--project requires a value".to_string());
                }
                project_name = Some(rest[i].clone());
            }
            "--limit" => {
                i += 1;
                if i >= rest.len() {
                    return Err("--limit requires a number".to_string());
                }
                limit = rest[i]
                    .parse()
                    .map_err(|_| format!("Invalid limit: {}", rest[i]))?;
            }
            "--page" => {
                i += 1;
                if i >= rest.len() {
                    return Err("--page requires a number".to_string());
                }
                page = rest[i]
                    .parse()
                    .map_err(|_| format!("Invalid page: {}", rest[i]))?;
            }
            "--json" => {
                json_output = true;
            }
            "--token" | "-t" => {
                i += 1;
                if i >= rest.len() {
                    return Err("--token requires a value".to_string());
                }
                session_token = Some(rest[i].clone());
            }
            "-o" | "--output" => {
                i += 1;
                if i >= rest.len() {
                    return Err("--output requires a path".to_string());
                }
                output = Some(PathBuf::from(&rest[i]));
            }
            _ if rest[i].starts_with('-') => {
                return Err(format!("Unknown option: {}", rest[i]));
            }
            _ => {
                positional.push(rest[i].clone());
            }
        }
        i += 1;
    }

    let command = match subcmd.as_str() {
        "projects" => CloudCommand::Projects,
        "sessions" | "list" => CloudCommand::Sessions { limit, page, json: json_output },
        "info" | "details" => {
            let session_id = positional
                .first()
                .ok_or("Missing SESSION_ID argument")?
                .clone();
            CloudCommand::Info { session_id }
        }
        "download" | "fetch" => {
            let session_id = positional
                .first()
                .ok_or("Missing SESSION_ID argument")?
                .clone();
            CloudCommand::Download { session_id, output }
        }
        "help" | "--help" | "-h" => {
            return Err("show_help".to_string());
        }
        _ => {
            return Err(format!("Unknown cloud command: '{}'. Use: projects, sessions, info, download", subcmd));
        }
    };

    Ok(CloudOptions {
        command,
        project_name,
        session_token,
    })
}

pub fn print_cloud_help() {
    eprintln!("Usage: livekit-analyzer cloud <command> [options]");
    eprintln!();
    eprintln!("Fetch sessions and observability data from LiveKit Cloud.");
    eprintln!("Credentials are read from ~/.livekit/cli-config.yaml (shared with `lk` CLI).");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  projects                      List configured projects");
    eprintln!("  sessions                      List recent sessions");
    eprintln!("  info <SESSION_ID>             Show session details");
    eprintln!("  download <SESSION_ID>         Download observability data (experimental)");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -p, --project <NAME>          Project name (default: from config)");
    eprintln!("  -t, --token <TOKEN>           Session token for download (from browser cookie)");
    eprintln!("  --limit <N>                   Max sessions to list (default: 20)");
    eprintln!("  --page <N>                    Page number (default: 0)");
    eprintln!("  --json                        Output as JSON");
    eprintln!("  -o, --output <DIR>            Output directory for download");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  livekit-analyzer cloud projects");
    eprintln!("  livekit-analyzer cloud sessions --project my-project --limit 10");
    eprintln!("  livekit-analyzer cloud sessions --json");
    eprintln!("  livekit-analyzer cloud info RM_bMvTTdAVKvmW");
    eprintln!("  livekit-analyzer cloud download RM_bMvTTdAVKvmW -o ./session-data");
}

pub fn run(options: CloudOptions) -> Result<()> {
    let config = load_config()?;

    match options.command {
        CloudCommand::Projects => {
            cmd_projects(&config);
            Ok(())
        }
        CloudCommand::Sessions { limit, page, json } => {
            let project = resolve_project(&config, options.project_name.as_deref())?;
            cmd_sessions(project, limit, page, json)
        }
        CloudCommand::Info { session_id } => {
            let project = resolve_project(&config, options.project_name.as_deref())?;
            cmd_info(project, &session_id)
        }
        CloudCommand::Download { session_id, output } => {
            let project = resolve_project(&config, options.project_name.as_deref())?;
            let output_dir = output.unwrap_or_else(|| {
                PathBuf::from(format!("observability-{}", session_id))
            });
            cmd_download(project, &session_id, &output_dir, options.session_token.as_deref())
        }
    }
}

fn cmd_projects(config: &CliConfig) {
    let default = config.default_project.as_deref().unwrap_or("");
    println!("{:<25} {:<18} {}", "NAME", "PROJECT_ID", "URL");
    println!("{}", "-".repeat(80));
    for p in &config.projects {
        let marker = if p.name == default { " *" } else { "" };
        let pid = p.project_id.as_deref().unwrap_or("-");
        println!("{:<25} {:<18} {}{}", p.name, pid, p.url, marker);
    }
    eprintln!("\n* = default project");
}

fn cmd_sessions(project: &ProjectConfig, limit: u32, page: u32, json: bool) -> Result<()> {
    let project_id = project
        .project_id
        .as_deref()
        .with_context(|| format!("Project '{}' has no project_id in config", project.name))?;

    let token = generate_token(project)?;
    let resp = list_sessions(&token, project_id, limit, page)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&resp.sessions)?);
        return Ok(());
    }

    if resp.sessions.is_empty() {
        eprintln!("No sessions found for project '{}'.", project.name);
        return Ok(());
    }

    println!(
        "{:<20} {:<50} {:<22} {:>6}",
        "SESSION_ID", "ROOM_NAME", "CREATED", "USERS"
    );
    println!("{}", "-".repeat(105));

    for s in &resp.sessions {
        let created = format_timestamp(&s.created_at);
        let room = if s.room_name.len() > 48 {
            format!("{}...", &s.room_name[..45])
        } else {
            s.room_name.clone()
        };
        println!(
            "{:<20} {:<50} {:<22} {:>6}",
            s.session_id, room, created, s.participants()
        );
    }

    eprintln!(
        "\n{} sessions (page {}, limit {})",
        resp.sessions.len(),
        page,
        limit
    );
    Ok(())
}

fn cmd_info(project: &ProjectConfig, session_id: &str) -> Result<()> {
    let project_id = project
        .project_id
        .as_deref()
        .with_context(|| format!("Project '{}' has no project_id in config", project.name))?;

    let token = generate_token(project)?;
    let detail = get_session_detail(&token, project_id, session_id)?;

    println!("Session: {}", detail.room_id);
    println!("Room:    {}", detail.room_name);
    println!("Start:   {}", format_timestamp(&detail.start_time));
    println!("End:     {}", format_timestamp(&detail.end_time));
    println!("Users:   {}", json_to_u64(&detail.num_participants));
    println!("Minutes: {}", json_to_u64(&detail.connection_minutes));

    if !detail.participants.is_empty() {
        println!("\nParticipants:");
        println!(
            "  {:<30} {:<20} {:<15} {:<15} {}",
            "IDENTITY", "NAME", "DEVICE", "SDK", "REGION"
        );
        for p in &detail.participants {
            println!(
                "  {:<30} {:<20} {:<15} {:<15} {}",
                truncate(&p.identity, 28),
                truncate(&p.name, 18),
                truncate(&p.device_model, 13),
                truncate(&p.sdk_version, 13),
                p.region
            );
        }
    }

    Ok(())
}

fn cmd_download(project: &ProjectConfig, session_id: &str, output_dir: &PathBuf, session_token: Option<&str>) -> Result<()> {
    let project_id = project
        .project_id
        .as_deref()
        .with_context(|| format!("Project '{}' has no project_id in config", project.name))?;

    let jwt_token = generate_token(project)?;
    let session_token = resolve_session_token(session_token)?;

    eprintln!("Downloading session {} from project '{}'...", session_id, project.name);
    download_session(&jwt_token, &session_token, project_id, session_id, output_dir)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_timestamp(ts: &str) -> String {
    // Input: "2026-02-23T15:30:00Z" or similar
    // Output: "2026-02-23 15:30:00"
    ts.replace('T', " ").replace('Z', "").chars().take(19).collect()
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max.saturating_sub(3)])
    } else {
        s.to_string()
    }
}
