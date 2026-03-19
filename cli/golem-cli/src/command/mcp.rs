use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub enum McpSubcommand {
    /// Start MCP server
    Serve {
        /// Port to listen on
        #[arg(long, default_value = "1232")]
        port: u16,
    },
}
