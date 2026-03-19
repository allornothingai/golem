use crate::command::mcp::McpSubcommand;
use crate::context::Context;
use crate::command_handler::{CommandHandler, CommandHandlerHooks};
use crate::log::{start_capturing, stop_capturing};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, stdin, stdout};
use serde_json::{json, Value};
use std::future::Future;

pub struct McpCommandHandler {
    ctx: Arc<Context>,
}

struct McpHooks;

impl CommandHandlerHooks for McpHooks {
    #[cfg(feature = "server-commands")]
    fn handler_server_commands(
        &self,
        _ctx: Arc<Context>,
        _subcommand: crate::command::server::ServerSubcommand,
    ) -> impl Future<Output = anyhow::Result<()>> {
        async move { unimplemented!() }
    }

    #[cfg(feature = "server-commands")]
    fn run_server() -> impl Future<Output = anyhow::Result<()>> + Send {
        async move { unimplemented!() }
    }

    #[cfg(feature = "server-commands")]
    fn override_verbosity(verbosity: clap_verbosity_flag::Verbosity) -> clap_verbosity_flag::Verbosity {
        verbosity
    }

    #[cfg(feature = "server-commands")]
    fn override_pretty_mode() -> bool {
        false
    }
}

impl McpCommandHandler {
    pub fn new(ctx: Arc<Context>) -> Self {
        Self { ctx }
    }

    pub fn handle_command(
        &self,
        subcommand: McpSubcommand,
    ) -> std::pin::Pin<Box<dyn Future<Output = anyhow::Result<()>> + '_>> {
        Box::pin(async move {
            match subcommand {
                McpSubcommand::Serve { port: _ } => {
                    self.cmd_serve().await
                }
            }
        })
    }

    async fn cmd_serve(&self) -> anyhow::Result<()> {
        let stdin = stdin();
        let mut reader = BufReader::new(stdin).lines();
        let mut stdout = stdout();

        while let Some(line) = reader.next_line().await? {
            let request: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let id = request.get("id").cloned().unwrap_or(Value::Null);
            let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");

            let response = match method {
                "initialize" => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {},
                            "resources": {}
                        },
                        "serverInfo": {
                            "name": "golem-cli-mcp",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }
                }),
                "resources/list" => {
                    let mut resources = Vec::new();
                    for entry in walkdir::WalkDir::new(".")
                        .max_depth(3)
                        .into_iter()
                        .filter_map(|e| e.ok()) {
                        if entry.file_name() == "golem.yaml" {
                            let path = entry.path();
                            resources.push(json!({
                                "uri": format!("file://{}", path.display()),
                                "name": format!("Golem Manifest: {}", path.display()),
                                "description": format!("Application manifest at {}", path.display()),
                                "mimeType": "application/x-yaml"
                            }));
                        }
                    }
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "resources": resources }
                    })
                }
                "resources/read" => {
                    let uri = request.get("params").and_then(|p| p.get("uri")).and_then(|v| v.as_str()).unwrap_or("");
                    if let Some(path_str) = uri.strip_prefix("file://") {
                        match std::fs::read_to_string(path_str) {
                            Ok(content) => json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "contents": [{
                                        "uri": uri,
                                        "mimeType": "application/x-yaml",
                                        "text": content
                                    }]
                                }
                            }),
                            Err(e) => json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": { "code": -32000, "message": format!("Failed to read manifest: {}", e) }
                            }),
                        }
                    } else {
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32602, "message": "Unsupported URI scheme" }
                        })
                    }
                }
                "tools/list" => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": [
                            {
                                "name": "golem_cli",
                                "description": "Execute any Golem CLI command. Pass the arguments as a list of strings.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "args": {
                                            "type": "array",
                                            "items": { "type": "string" },
                                            "description": "The command line arguments (e.g. ['agent', 'list', '--component-name', 'my-comp'])"
                                        }
                                    },
                                    "required": ["args"]
                                }
                            }
                        ]
                    }
                }),
                "tools/call" => {
                    let tool_name = request.get("params").and_then(|p| p.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                    if tool_name == "golem_cli" {
                        let args = request.get("params")
                            .and_then(|p| p.get("arguments"))
                            .and_then(|a| a.get("args"))
                            .and_then(|v| v.as_array());
                        
                        if let Some(args) = args {
                            let mut args_vec: Vec<String> = args.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect();

                            if args_vec.first().map(|s| s.as_str()) != Some("golem-cli") {
                                args_vec.insert(0, "golem-cli".to_string());
                            }

                            start_capturing();
                            let _exit_code = CommandHandler::handle_args(args_vec, Arc::new(McpHooks)).await;
                            let output = stop_capturing();

                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "content": [{
                                        "type": "text",
                                        "text": output.join("\n")
                                    }]
                                }
                            })
                        } else {
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": { "code": -32602, "message": "Missing or invalid 'args' parameter" }
                            })
                        }
                    } else {
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32601, "message": "Method not found" }
                        })
                    }
                }
                "notifications/initialized" => continue,
                _ => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": "Method not found" }
                }),
            };

            let response_line = serde_json::to_string(&response)? + "\n";
            stdout.write_all(response_line.as_bytes()).await?;
            stdout.flush().await?;
        }

        Ok(())
    }
}
