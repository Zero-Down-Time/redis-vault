# Rust Project Guidelines

## Dependency Management

**Minimize dependencies:**
- Prefer std library when possible
- Use `cargo-udeps` to find unused dependencies

**Keep dependencies updated:**
- `cargo update`

**Security-focused crates:**
- `cargo-audit` - check for known vulnerabilities
- `cargo-deny` - lint dependencies for security/licensing

## Development Tools

**Makefile and Jenkinsfile**:
- `cargo fmt` - enforce formatting
- `cargo clippy` - strict linting
- `cargo nextest` - run tests
- `cargo bench` - benchmarking

## Security Practices

**Code safety:**
- Minimize `unsafe` blocks - document why each is needed
- Enable `#![forbid(unsafe_code)]` when possible

**Input validation:**
- Validate all external input (files, network, CLI args)
- Use type system for validation (newtypes, enums)
- Sanitize before logging sensitive data

**Secrets & credentials:**
- Never hardcode secrets
- Use environment variables or secret managers
- Add `.env`, `*.key`, `*.pem` to `.gitignore`

**Dependencies:**
- Pin versions for production builds
- Review code of small/new crates before adding
- Prefer well-maintained crates with recent commits
- Check crate popularity and audit history

## Error Handling

- Use `Result<T, E>` for recoverable errors
- Use `Option<T>` for absence of value
- Prefer `?` operator over `.unwrap()`
- Add context with `anyhow` or `thiserror` for errors
- Log errors appropriately - avoid exposing internals

## Performance Notes

- Profile before optimizing: `cargo flamegraph`
- Use `&str` over `String` when possible
- Prefer iterators over collecting to vectors
- Consider `Cow<str>` for conditional ownership
