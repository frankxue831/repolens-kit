# Security Policy

## Supported versions

RepoLens is pre-1.0. Only the latest commit on `main` gets security fixes.

## Reporting a vulnerability

Please don't report security issues in public issues or pull requests.

Report them privately through GitHub's **Report a vulnerability** button on this repository's Security tab ([private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)).

Please include the affected command or MCP tool, steps to reproduce, and the impact you expect. This is a one-person project, so expect an initial reply within about a week. Fixes go out on a best-effort basis.

## Scope

RepoLens reads local repositories, writes files under `.repolens/`, and runs an MCP server over standard input and output. Issues of particular interest include:

- writing or reading outside the target repository or `.repolens/`
- path traversal through pack, memory, or eval-run inputs
- MCP tools exposing file contents they shouldn't
- crafted repositories causing crashes, hangs, or excessive resource use during scan
