---
description: writing ways that describe patterns and intentions rather than referencing specific tool names, MCP servers, or CLI commands
vocabulary: tool agnostic vendor neutral portable abstract intention pattern mcp cli specific
files: \.claude/(hooks/)?ways/.*way\.md$
scope: agent, subagent
refire: 0.15
---
<!-- epistemic: premise -->
# Tool-Agnostic Ways

Ways should describe patterns and intentions, not reference specific tool names, MCP servers, CLI commands, or API endpoints. Different users may have different tools that serve the same purpose.

## The Principle

Write what needs to happen, not how to call it.

| Too Specific | Tool-Agnostic |
|---|---|
| "Call `mcp__google-workspace__manage_email`" | "Check email across configured accounts" |
| "Use `gh issue create`" | "Create an issue in the project tracker" |
| "Query the Confluence MCP for page content" | "Search the team wiki for relevant pages" |
| "Run `osv-scanner --lockfile=package-lock.json`" | "Scan dependencies for known vulnerabilities" |

## Why This Matters

- **Portability.** A teammate may use Outlook instead of Gmail, Bitbucket instead of GitHub, Linear instead of Jira. The workflow pattern is the same; the tool binding differs.
- **Durability.** Tool names change, MCP servers get renamed, APIs evolve. Patterns outlast implementations.
- **Composability.** When ways describe intentions, the agent can match them to whatever tools are actually available in the current session.

## When Specificity Is Appropriate

Tool names belong in **skills** (which bind to specific tools via `allowed-tools`) and in **macros** (which execute specific commands). Ways describe the *what* and *why*; skills and macros handle the *how*.

The exception: if a way exists specifically to guide usage of a particular tool (e.g., a "git commit formatting" way), naming that tool is obviously fine. The rule applies to ways that describe workflows spanning multiple tools.

## Common Rationalizations

| Rationalization | Counter |
|---|---|
| "Everyone on the team uses the same tools" | Teams change. New members bring different setups. Ways should onboard, not exclude. |
| "Being specific is clearer" | Specific tool calls belong in skills or macros. Ways guide judgment, not keystrokes. |
| "The agent needs to know which tool to call" | The agent discovers available tools from the session. It maps intentions to tools at runtime. |
