# Security Policy

## Supported Versions

Security fixes target the latest released version of `codex-harness`.

## Reporting a Vulnerability

Please report vulnerabilities privately through GitHub's private vulnerability reporting if it is enabled on the repository. If it is not enabled, open a minimal issue that states a security contact is needed without publishing exploit details.

Do not include secrets, customer data, private repository contents, or raw production payloads in public issues.

## Security Expectations

`codex-harness` is a local CLI that writes harness documentation into target repositories. It should:

- avoid printing secret values
- reject upstream request paths containing parent-directory traversal
- preserve existing target files by default
- keep scratch planning outside repositories unless tracked mode is explicitly requested
