import 'builder.just'
import 'common.just'
import 'git.just'

prepare:
  cargo fetch

# requires 1.45 [arg('target', pattern='(release)?')]
lint target="":
  #!/usr/bin/env sh
  if [ "{{ target }}" == "release" ]; then
    cargo clippy --workspace --locked --release
  else
    cargo clippy --workspace --locked
  fi
  cargo deny check -s

fmt:
  cargo fmt

# requires 1.45 [arg('target', pattern='(release)?')]
build target="":
  #!/usr/bin/env sh
  if [ "{{ target }}" == "release" ]; then
    cargo auditable build --locked --release
  else
    cargo auditable build --locked
  fi

# requires 1.45 [arg('target', pattern='(release)?')]
test target="":
  #!/usr/bin/env sh
  if [ "{{ target }}" == "release" ]; then
    cargo test --locked --release
  else
    cargo test --locked
  fi

clean:
  cargo clean

# requires 1.45 [arg('version', pattern='^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$')]
cut-release version: && (_addCommitTagPush version "Cargo.*")
  @sed -i -e "s/^version = \".*/version = \"{{ trim_start_match(version, "v") }}\"/" Cargo.toml
  @cargo update -w
