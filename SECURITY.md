# Security Policy

## Supported versions

This project is pre-1.0. Security fixes will land on the latest main branch.

## Reporting a vulnerability

Please do **not** open public issues for security problems.

Report vulnerabilities privately to the maintainer through GitHub Security Advisories or another private channel you control.

When reporting, include:

- affected version or commit
- reproduction steps
- impact
- any suggested mitigation

## Secret handling

- Never commit API keys, tokens, recordings, transcripts, or local config files containing secrets.
- Prefer environment variables for hosted-provider credentials.
- Keep example config files secret-free.
