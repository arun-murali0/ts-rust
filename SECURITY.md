# Security Policy

## Reporting a vulnerability

Please do not disclose security-sensitive vulnerabilities in public issues.

Use the repository's private security reporting mechanism when available. Include:
- affected version/commit
- minimal reproduction
- impact
- relevant environment information

Do not include secrets or unnecessary personal data.

## Security expectations

The checker processes source code that may be untrusted. Changes should consider:
- parser and checker denial-of-service cases
- unbounded recursion or type expansion
- unsafe filesystem/process access
- dependency vulnerabilities
- accidental secret disclosure

Security fixes should include regression coverage when practical.
