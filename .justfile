# Rust owns the root namespace
import '.ci/rust.just'

# container image tasks
mod container '.ci/container.just'

toolchain := "rust"

default: build

# scan debug build using grype
scan: build
  grype file:target/debug/redis-vault
