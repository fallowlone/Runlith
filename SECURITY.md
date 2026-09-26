# Security Policy

## Reporting a vulnerability

Please report security vulnerabilities **privately**, using GitHub's
private vulnerability reporting feature: go to the repository's
**Security** tab and select **"Report a vulnerability."** This opens a
private advisory visible only to the maintainer and does not create a
public issue.

**Do not open a public GitHub issue for a security vulnerability.** Public
issues are visible to everyone, including before a fix is available.

Private vulnerability reporting must be enabled on the repository by the
maintainer before this channel is available; see
[docs/development/PUBLICATION_RUNBOOK.md](docs/development/PUBLICATION_RUNBOOK.md)
for that setup step. If reporting is not yet enabled and you believe you
have found a vulnerability, please wait rather than filing a public issue.

When reporting, please include enough detail to reproduce the issue
(affected component, steps, expected vs. actual behavior). **Never include
secrets, API keys, tokens, or credentials in a report** — describe where
they are and how they are exposed instead of pasting the values.

## Supported versions

Only the latest `main` branch is supported. Runlith has not yet reached a
public release; there are no maintained release branches or backported
security fixes for older commits.

## Bug bounty

There is no bug bounty program at this time.
