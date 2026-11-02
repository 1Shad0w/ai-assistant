# mcp-client

**Rama:** `feat/03-mcp-orchestrator`

## Objetivos técnicos
- Lanzar y supervisar servidores MCP como subprocesos (stdio, JSON-RPC 2.0).
- Implementar `McpClient`: `initialize`, `tools/list`, `tools/call`.
- Enrutar herramientas entre varios servidores (Google Workspace, Puppeteer, filesystem, OmniRoute) según `.agents/mcp_config.json`.

## Dependencias previstas
`tokio` (process, io-util), `serde_json`, `core-types`, `tracing`.

## Plataforma objetivo
Windows, Linux y macOS (requiere Node.js para los servidores).
