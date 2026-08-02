//! Read-only, stdio LSP bridge exposed as a namespaced agent tool.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::error::AgentError;
use crate::integrations::plugin::manifest::LspServerEntry;
use crate::tools::api::{PermissionDecision, Tool, ToolContext, ToolDefinition, ToolOutput};

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub struct LspTool {
    definition: ToolDefinition,
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    extension_to_language: HashMap<String, String>,
}

impl LspTool {
    pub fn new(
        tool_name: String,
        server_name: &str,
        entry: LspServerEntry,
        plugin_root: &Path,
        mut plugin_env: HashMap<String, String>,
    ) -> Result<Self, AgentError> {
        if entry.transport != "stdio" {
            return Err(AgentError::Config(format!(
                "LSP server `{server_name}` uses unsupported transport `{}`",
                entry.transport
            )));
        }
        if entry.command.trim().is_empty() {
            return Err(AgentError::Config(format!(
                "LSP server `{server_name}` must declare a command"
            )));
        }
        if entry.extension_to_language.is_empty() {
            return Err(AgentError::Config(format!(
                "LSP server `{server_name}` must declare extensionToLanguage"
            )));
        }
        if entry
            .extension_to_language
            .iter()
            .any(|(extension, language)| extension.trim().is_empty() || language.trim().is_empty())
        {
            return Err(AgentError::Config(format!(
                "LSP server `{server_name}` has an empty extension or language mapping"
            )));
        }
        if entry.env.keys().any(|key| key.is_empty() || key.contains('=') || key.contains('\0')) {
            return Err(AgentError::Config(format!(
                "LSP server `{server_name}` has an invalid environment key"
            )));
        }
        plugin_env.extend(entry.env.into_iter().map(|(key, value)| {
            (key, value.replace("${PLUGIN_ROOT}", &plugin_root.to_string_lossy()))
        }));
        let command = resolve_command(&entry.command, plugin_root);
        let args = entry
            .args
            .into_iter()
            .map(|argument| argument.replace("${PLUGIN_ROOT}", &plugin_root.to_string_lossy()))
            .collect();
        Ok(Self {
            definition: ToolDefinition {
                name: tool_name,
                description: format!(
                    "Query the `{server_name}` language server for hover, definition, references, document symbols, or diagnostics. Positions are zero-based."
                ),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "operation": {
                            "type": "string",
                            "enum": ["hover", "definition", "references", "document_symbols", "diagnostics"]
                        },
                        "filePath": {"type": "string"},
                        "line": {"type": "integer", "minimum": 0},
                        "character": {"type": "integer", "minimum": 0}
                    },
                    "required": ["operation", "filePath"],
                    "additionalProperties": false
                }),
            },
            command,
            args,
            env: plugin_env,
            extension_to_language: entry.extension_to_language,
        })
    }

    async fn query(&self, arguments: &Value, context: &ToolContext) -> Result<Value, AgentError> {
        let timeout = context.timeout.unwrap_or(Duration::from_secs(30));
        tokio::time::timeout(timeout, self.query_inner(arguments, context))
            .await
            .map_err(|_| self.failure("language server request timed out"))?
    }

    async fn query_inner(
        &self,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<Value, AgentError> {
        let operation = arguments
            .get("operation")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::Validation("operation is required".into()))?;
        let file_path = arguments
            .get("filePath")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::Validation("filePath is required".into()))?;
        let path = resolve_workspace_path(&context.cwd, file_path)?;
        let metadata = tokio::fs::metadata(&path).await.map_err(|error| {
            AgentError::Validation(format!("cannot inspect {}: {error}", path.display()))
        })?;
        if metadata.len() > context.max_file_read_bytes as u64 {
            return Err(AgentError::Validation(format!(
                "file exceeds {} byte LSP limit",
                context.max_file_read_bytes
            )));
        }
        let text = tokio::fs::read_to_string(&path).await.map_err(|error| {
            AgentError::Validation(format!("cannot read {}: {error}", path.display()))
        })?;
        let language_id = self.language_id(&path)?;
        let file_uri = url::Url::from_file_path(&path)
            .map_err(|_| AgentError::Validation("file path cannot be represented as a URI".into()))?
            .to_string();
        let root_uri = url::Url::from_directory_path(&context.cwd)
            .map_err(|_| AgentError::Validation("cwd cannot be represented as a URI".into()))?
            .to_string();

        let mut child = self.spawn(context)?;
        let mut stdin = child.stdin.take().ok_or_else(|| self.failure("stdin unavailable"))?;
        let stdout = child.stdout.take().ok_or_else(|| self.failure("stdout unavailable"))?;
        let mut stdout = BufReader::new(stdout);
        let request_id = NEXT_REQUEST_ID.fetch_add(2, Ordering::Relaxed);
        send_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "initialize",
                "params": {
                    "processId": null,
                    "rootUri": root_uri,
                    "capabilities": {},
                    "clientInfo": {"name": "telos-agent", "version": env!("CARGO_PKG_VERSION")}
                }
            }),
        )
        .await
        .map_err(|error| self.failure(&format!("initialize write failed: {error}")))?;
        read_response(&mut stdout, &mut stdin, request_id)
            .await
            .map_err(|error| self.failure(&error))?;
        send_message(&mut stdin, &json!({"jsonrpc":"2.0","method":"initialized","params":{}}))
            .await
            .map_err(|error| self.failure(&format!("initialized write failed: {error}")))?;
        send_message(
            &mut stdin,
            &json!({
                "jsonrpc":"2.0",
                "method":"textDocument/didOpen",
                "params":{"textDocument":{"uri":file_uri,"languageId":language_id,"version":1,"text":text}}
            }),
        )
        .await
        .map_err(|error| self.failure(&format!("didOpen write failed: {error}")))?;

        let query_id = request_id + 1;
        let line = arguments.get("line").and_then(Value::as_u64).unwrap_or(0);
        let character = arguments.get("character").and_then(Value::as_u64).unwrap_or(0);
        let (method, params) = operation_request(operation, &file_uri, line, character)?;
        send_message(
            &mut stdin,
            &json!({"jsonrpc":"2.0","id":query_id,"method":method,"params":params}),
        )
        .await
        .map_err(|error| self.failure(&format!("query write failed: {error}")))?;

        let result = read_response(&mut stdout, &mut stdin, query_id)
            .await
            .map_err(|error| self.failure(&error));
        let _ = shutdown(&mut child, &mut stdin).await;
        result
    }

    fn language_id(&self, path: &Path) -> Result<&str, AgentError> {
        let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or("");
        self.extension_to_language
            .get(&format!(".{extension}"))
            .or_else(|| self.extension_to_language.get(extension))
            .map(String::as_str)
            .ok_or_else(|| {
                AgentError::Validation(format!("no language mapping for extension `{extension}`"))
            })
    }

    fn spawn(&self, context: &ToolContext) -> Result<Child, AgentError> {
        let mut command = Command::new(&self.command);
        command
            .args(&self.args)
            .current_dir(&context.cwd)
            .env_clear()
            .envs(&self.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        command.spawn().map_err(|error| {
            self.failure(&format!("failed to start language server `{}`: {error}", self.command))
        })
    }

    fn failure(&self, message: &str) -> AgentError {
        AgentError::ToolExecution { tool: self.definition.name.clone(), message: message.into() }
    }
}

#[async_trait]
impl Tool for LspTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn is_concurrency_safe(&self, _arguments: &Value) -> bool {
        true
    }

    async fn check_permission(
        &self,
        _arguments: &Value,
        _context: &ToolContext,
    ) -> Result<PermissionDecision, AgentError> {
        Ok(PermissionDecision::Allow)
    }

    async fn invoke(
        &self,
        arguments: Value,
        context: ToolContext,
    ) -> Result<ToolOutput, AgentError> {
        self.query(&arguments, &context).await.map(ToolOutput::json)
    }
}

fn operation_request(
    operation: &str,
    uri: &str,
    line: u64,
    character: u64,
) -> Result<(&'static str, Value), AgentError> {
    let document = json!({"uri": uri});
    let position = json!({"line": line, "character": character});
    match operation {
        "hover" => Ok(("textDocument/hover", json!({"textDocument":document,"position":position}))),
        "definition" => {
            Ok(("textDocument/definition", json!({"textDocument":document,"position":position})))
        }
        "references" => Ok((
            "textDocument/references",
            json!({"textDocument":document,"position":position,"context":{"includeDeclaration":true}}),
        )),
        "document_symbols" => Ok(("textDocument/documentSymbol", json!({"textDocument":document}))),
        "diagnostics" => Ok(("textDocument/diagnostic", json!({"textDocument":document}))),
        _ => Err(AgentError::Validation(format!("unsupported LSP operation `{operation}`"))),
    }
}

async fn send_message(stdin: &mut ChildStdin, value: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    stdin.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes()).await?;
    stdin.write_all(&body).await?;
    stdin.flush().await
}

async fn read_response(
    stdout: &mut BufReader<ChildStdout>,
    stdin: &mut ChildStdin,
    id: u64,
) -> Result<Value, String> {
    loop {
        let message = read_message(stdout).await.map_err(|error| error.to_string())?;
        if message.get("id").and_then(Value::as_u64) != Some(id) {
            respond_to_server_request(stdin, &message).await?;
            continue;
        }
        if let Some(error) = message.get("error") {
            return Err(format!("language server returned error: {error}"));
        }
        return Ok(message.get("result").cloned().unwrap_or(Value::Null));
    }
}

async fn respond_to_server_request(stdin: &mut ChildStdin, message: &Value) -> Result<(), String> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(request_id) = message.get("id").cloned() else {
        return Ok(());
    };
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let response = match method {
        "workspace/configuration" => {
            let count = params.get("items").and_then(Value::as_array).map_or(0, Vec::len);
            json!({"jsonrpc":"2.0","id":request_id,"result":vec![Value::Null; count]})
        }
        "window/workDoneProgress/create"
        | "client/registerCapability"
        | "client/unregisterCapability"
        | "window/showMessageRequest" => {
            json!({"jsonrpc":"2.0","id":request_id,"result":Value::Null})
        }
        "workspace/workspaceFolders" => {
            json!({"jsonrpc":"2.0","id":request_id,"result":Value::Null})
        }
        "workspace/applyEdit" => json!({
            "jsonrpc":"2.0",
            "id":request_id,
            "result":{"applied":false,"failureReason":"telos-agent LSP bridge is read-only"}
        }),
        _ => json!({
            "jsonrpc":"2.0",
            "id":request_id,
            "error":{"code":-32601,"message":format!("unsupported server request `{method}`")}
        }),
    };
    send_message(stdin, &response).await.map_err(|error| error.to_string())
}

async fn read_message(stdout: &mut BufReader<ChildStdout>) -> std::io::Result<Value> {
    let mut content_length = None;
    loop {
        let mut header = String::new();
        if stdout.read_line(&mut header).await? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "language server closed stdout",
            ));
        }
        let header = header.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some(value) = header.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let length = content_length.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length header")
    })?;
    if length > 16 * 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "language server response exceeds 16 MiB",
        ));
    }
    let mut body = vec![0; length];
    stdout.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map_err(std::io::Error::other)
}

async fn shutdown(
    child: &mut Child,
    stdin: &mut ChildStdin,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    send_message(stdin, &json!({"jsonrpc":"2.0","id":id,"method":"shutdown","params":null}))
        .await?;
    send_message(stdin, &json!({"jsonrpc":"2.0","method":"exit","params":null})).await?;
    let _ = child.kill().await;
    Ok(())
}

fn resolve_command(command: &str, plugin_root: &Path) -> String {
    let command = command.replace("${PLUGIN_ROOT}", &plugin_root.to_string_lossy());
    let path = Path::new(&command);
    if path.is_absolute()
        || (!command.starts_with('.') && !command.contains('/') && !command.contains('\\'))
    {
        command
    } else {
        plugin_root.join(path).to_string_lossy().into_owned()
    }
}

fn resolve_workspace_path(cwd: &Path, requested: &str) -> Result<PathBuf, AgentError> {
    let joined = if Path::new(requested).is_absolute() {
        PathBuf::from(requested)
    } else {
        cwd.join(requested)
    };
    let canonical_cwd = std::fs::canonicalize(cwd).map_err(|error| {
        AgentError::Validation(format!("cannot resolve cwd {}: {error}", cwd.display()))
    })?;
    let canonical = std::fs::canonicalize(&joined).map_err(|error| {
        AgentError::Validation(format!("cannot resolve {}: {error}", joined.display()))
    })?;
    if !canonical.starts_with(&canonical_cwd) {
        return Err(AgentError::PermissionDenied(format!(
            "LSP path {} is outside workspace {}",
            canonical.display(),
            canonical_cwd.display()
        )));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::api::Tool;

    #[test]
    fn maps_only_read_only_operations() {
        assert_eq!(operation_request("hover", "file:///x", 1, 2).unwrap().0, "textDocument/hover");
        assert!(operation_request("execute_command", "file:///x", 0, 0).is_err());
    }

    #[test]
    fn rejects_paths_outside_workspace() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        assert!(resolve_workspace_path(root.path(), outside.path().to_str().unwrap()).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn performs_stdio_initialize_and_hover_roundtrip() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("example.rs");
        std::fs::write(&source, "fn main() {}\n").unwrap();
        let server = directory.path().join("fake-lsp.sh");
        std::fs::write(
            &server,
            r#"#!/bin/sh
read_message() {
  length=""
  while IFS= read -r line; do
    line=$(printf '%s' "$line" | tr -d '\r')
    [ -z "$line" ] && break
    case "$line" in Content-Length:*) length=$(printf '%s' "$line" | cut -d: -f2 | tr -d ' ') ;; esac
  done
  body=$(dd bs=1 count="$length" 2>/dev/null)
}
respond() {
  response="$1"
  printf 'Content-Length: %s\r\n\r\n%s' "${#response}" "$response"
}
read_message
id=$(printf '%s' "$body" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
respond "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"capabilities\":{}}}"
read_message
respond '{"jsonrpc":"2.0","id":900,"method":"workspace/configuration","params":{"items":[{"section":"fake"}]}}'
read_message
read_message
id=$(printf '%s' "$body" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
read_message
printf '%s' "$body" | grep -q '"id":900' || exit 2
respond "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"contents\":\"hover-ok\"}}"
"#,
        )
        .unwrap();
        std::fs::set_permissions(&server, std::fs::Permissions::from_mode(0o700)).unwrap();
        let tool = LspTool::new(
            "plugin__test__lsp__fake".into(),
            "fake",
            LspServerEntry {
                command: server.to_string_lossy().into_owned(),
                args: Vec::new(),
                extension_to_language: HashMap::from([(".rs".into(), "rust".into())]),
                transport: "stdio".into(),
                env: HashMap::new(),
            },
            directory.path(),
            crate::config::platform_base_env(),
        )
        .unwrap();
        let mut context = ToolContext::dummy();
        context.cwd = directory.path().to_path_buf();
        context.timeout = Some(Duration::from_secs(5));

        let output = tool
            .invoke(
                json!({"operation":"hover","filePath":"example.rs","line":0,"character":1}),
                context,
            )
            .await
            .unwrap();

        assert_eq!(output.content["contents"], "hover-ok");
    }
}
