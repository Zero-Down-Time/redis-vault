# ci-tools-lib

Shared CI/CD toolchain for ZeroDownTime: rootless Podman container builds, multi-arch (amd64/arm64), Grype + betterleaks scanning, AWS ECR Public publishing. Consumed via `git subtree` as `.ci/` inside downstream projects, plus a Jenkins shared-library reference (`@Library('ci-tools-lib')`).

## Core design rule

**All build logic lives in `*.just` modules. The Jenkinsfile and `vars/*.groovy` are glue only.**

Developers must reproduce full CI behaviour locally by running the same `just` recipes Jenkins runs. If logic leaks into Groovy, dev and CI diverge and "works on my machine" stops being meaningful.

- Acceptable in Groovy: `pipeline { ... }` declarative blocks, agent labels, `dir()`, `stash`/`unstash`, `withEnv`, `withCredentials`, `recordIssues`, `httpRequest`, `readJSON`/`writeJSON`, `currentBuild.description` flags, `env.*` reads.
- Not acceptable in Groovy: invoking scanners/linters/builders directly, computing versions, tagging/pushing images, rootfs extraction, file manipulation a developer would also need.
- A `sh` call in a Groovy wrapper should look like `sh "just <target> '${arg}'"` — nothing more.

## Architecture

Two layers:

1. **Just modules** (`*.just` at repo root, copied into consumers as `.ci/*.just`) — actual build logic. Composable via `import 'foo.just'`.
2. **Jenkins shared library** (`vars/*.groovy`) — thin per-stage wrappers + Jenkins-only concerns (changeset detection, recordIssues, PR build-file protection).

**Pipeline (declarative, in `vars/justContainer.groovy`):** Changeset → Prepare → Lint → Build → Test → Scan → Push → Cleanup.

- `currentBuild.description == 'SKIP'` is the cross-stage "no source changes, skip downstream" signal, set by `container.changeset` (the minimal first stage) when no changed files match `buildOnly` patterns (and neither the `forceBuild` config field nor the `FORCE_BUILD` build parameter is set). Gates Prepare, Lint, Build, Test, Scan, Push, cleanup — so the SKIP decision is made before any prep work runs.
- `Push` stage additionally gated by `not { changeRequest() }` — PRs never push.
- `justContainer` declares a `FORCE_BUILD` boolean parameter (default `false`), OR'd into `config.forceBuild` for the Changeset stage, so the skip gate can be overridden via the Jenkins "Build with Parameters" UI without editing config. (First build after introducing the parameter doesn't show the checkbox — registers from the second build onward.)

## Files

### Just modules

| File | Purpose |
|------|---------|
| `git.just` | `git_tag` / `git_branch` / `git_repo_name` derivation, `tag` with sanitized branch suffix when not on main/master, `arch` (overridable via `ARCH` env, default amd64) + arch validation (amd64/arm64), `_addCommitTagPush`, `_print-tag` (echoes `git_tag` — reachable as `container::_print-tag` via import), `cleanup-tags`, `ci-pull-upstream` |
| `common.just` | `scan-src` (source betterleaks). Imported by language modules so every language toolchain gets it. |
| `container.just` | `build`, `scan` (image betterleaks + grype), `ecr-login`, `push` (multi-arch manifest), `clean`, `rm-remote-untagged`, `create-repo`. Recipes that touch a registry take it as their **first positional argument** (`registry`). Public ECR (`public.ecr.aws/...`) and private ECR (`*.dkr.ecr.<region>.amazonaws.com`) auto-detected by URL shape. `build`/`scan`/`clean` take no registry and stay reachable directly. Consumers typically define `registry := "..."` in their root justfile and either pass it explicitly (`just container::push {{ registry }} <image>`) or wrap the recipes locally; the Jenkins glue propagates the `registry:` config field as the first positional. |
| `builder.just` | `update-builder` (build toolchain image), `use-builder <target>` (run target inside toolchain container, reused across pipeline stages via `buildah from --name local-<toolchain>-builder-$BUILD_TAG`; mounts repo root at `/app`; for `toolchain == "rust"` only, also mounts `$SCCACHE_DIR`/`~/.cache/sccache` at `/root/.cache/sccache`, and — when `CARGO_BUILD_MUSL` is set — injects an `ARCH`-derived `CARGO_BUILD_TARGET=<...>-alpine-linux-musl` into the container via `-e` so musl-qualified output paths never leak to host cargo; an explicit `CARGO_BUILD_TARGET` env wins), `clean-builder` (tear down the per-pipeline container; called from `post.cleanup` in `justContainer`). |
| `rust.just` | imports `common.just`; `prepare` (`cargo fetch --locked` — fails loudly if `Cargo.lock` drifts from the toolchain image's cargo instead of silently rewriting), `lint` (clippy + cargo-deny), `build [release]` (cargo auditable), `test`, `update-lock` (`cargo update -w`; meant to be invoked locally via `just use-builder update-lock` so the regenerated lock matches the Alpine cargo CI uses), `cut-release`. A consumer opts into a target-qualified Alpine musl build (artifacts under `target/<triple>/<profile>/`) with `export CARGO_BUILD_MUSL := "true"` (or env `CARGO_BUILD_MUSL=true`); `use-builder` then injects `CARGO_BUILD_TARGET=<x86_64|aarch64>-alpine-linux-musl` (per `ARCH`) **inside the builder only**, so plain `just build` on a non-Alpine dev host is unaffected. Requires `needBuilder`. Consumers must NOT `export CARGO_BUILD_TARGET` directly — it would hit host cargo and break local off-Alpine builds. Toolchain image (`Dockerfile.rust`) sets `RUSTC_WRAPPER=/usr/bin/sccache`, so the shared cache mount in `use-builder` accelerates Build/Test across stages and across pipeline runs on the same agent; `build` and `test` recipes print `sccache --show-stats` at the end when `RUSTC_WRAPPER` is set. |
| `python.just` | imports `common.just`; uv-based: `prepare` (uv sync --locked), `lint` (flake8), `build` (uv build), `test` (uv run pytest), `upload` (uv publish) |
| `gitops.just` | GitOps writeback: single `update` recipe (clone + idempotency + edit + rebase-retry push, optional PR mode). Commit message read from `$GITOPS_COMMIT_MESSAGE` (set by `vars/updateGitops.groovy`, which owns the default-message format). PR opening lives in `vars/gitea.groovy` (`gitea.openPullRequest`). Updates spec is a JSON file (`{ "<file>": { "<yq-path>": "<value>" } }`) so push-mode promotions reproduce locally. Tools required: `git`, `yq` (mikefarah), `jq`. |

### Jenkins shared library (`vars/`)

| File | Purpose |
|------|---------|
| `justContainer.groovy` | **Current entry point** — declarative pipeline composing per-stage helpers. Declares a `FORCE_BUILD` boolean build parameter (default `false`) which is OR'd into `config.forceBuild` when invoking Changeset. Calls `notify.start(config)` unconditionally at the end of the Changeset stage and `notify.end(config)` in `post.always`; both self-filter inside `notify` (on `config.notify`, the events list and the `SKIP` flag), so notifications fire only when `config.notify` is set and `notifySkipped` governs start and end symmetrically. |
| `container.groovy` | Per-stage helpers consumed by `justContainer.groovy`. Methods (invoked as `container.<stage>(config)` inside `script { }`): `changeset` (minimal first stage — gitea changeset, gate on `pathsChanged(buildOnly)` or `forceBuild` → sets `currentBuild.description = 'SKIP'` when nothing matches; does no prep work, so the skip decision precedes everything else); `prepare` (runs only when not SKIP — `protectBuildFiles`, creates `tmpDir`, `just update-builder` if `needBuilder`, optional `just prepare`); the optional `config.env` list (withEnv format, e.g. `['CARGO_BUILD_MUSL=true']` or `['ARCH=arm64']`) is applied via `withEnv` around the Prepare/Lint/Build/Test shells so consumers can set host-safe build env from the Jenkinsfile alone — read by `use-builder` and materialized inside the rust builder; `lint` (`just scan-src` + recordIssues, then `just lint`); `build` (`just container::build` — runs only when prepare did not SKIP); `test` (`just test` if defined); `scan` (`just container::scan` + grype/betterleaks recordIssues); `push` (`just container::push <registry>` + `rm-remote-untagged`; `registry` required); `clean` (`just container::clean`); `cleanBuilder` (no-op unless `needBuilder`; calls `just clean-builder` — invoked from `post.cleanup`). |
| `gitea.groovy` | Gitea REST client: parses `GIT_URL`, fetches PR files (`/pulls/N/files`) or commit diff (`/compare/base...head`), opens/reuses PRs (`openPullRequest`); `pathsChanged(files, patterns)` regex-matches |
| `protectBuildFiles.groovy` | On PRs only: `git checkout origin/<target> -- <files>` to overwrite CI files from target branch (defends against PRs modifying their own CI) |
| `quietCheckout.groovy` | Manual `git fetch/checkout --quiet` replicating Git plugin behaviour with less console noise |
| `notify.groovy` | Optional build-lifecycle notifications (`start`/`end`) via an **apprise-api sidecar**. `end` maps `currentBuild.currentResult` → event (`success`/`failure`/`unstable`/`aborted`) + apprise `type`, self-filtering on the `notify.events` list and the `SKIP` flag. POSTs one JSON event (`title`/`body`/`type`/`tag`) via `httpRequest`; endpoint resolves as explicit `notify.key` → `${url}/notify/${key}` (key mode — destinations/secrets stored server-side in apprise-api), else `notify.urls` → `${url}/notify` (stateless), else `${url}/notify/default` (apprise-api's conventional `default` key). Endpoint defaults to `env.APPRISE_API_URL`, overridable via `notify.url`; optional `notify.credentialsId` (Secret Text) → `Authorization: Bearer`. Whole body wrapped in `try/catch` — a notification failure never fails the build. A UI abort delivers a `FlowInterruptedException` to the first interruptible step in `post {}` (the notify `httpRequest`/credential binding), so the send is retried once on that exception (`_dispatch`/`_sendOnce`) to survive the abort. `'aborted'` must be in `notify.events` to fire on abort — not in the default `['success','failure']`. Off unless `config.notify` is set. |
| `updateGitops.groovy` | GitOps writeback wrapper. Validates args, writes the updates spec to a tmp JSON file, picks credential binding by repo URL scheme (SSH key via `sshagent`, HTTPS via `gitUsernamePassword` → `GIT_ASKPASS`), then `sh "just gitops::update ..."`. For PR mode, additionally runs `just gitops::pr-open` against Gitea. Returns `[sha, branch, prUrl]`. Consumer-called from their own Jenkinsfile (not part of `justContainer` stages). |
| `buildPodman.groovy` | **Deprecated** — Make-based equivalent of `justContainer`, kept for unmigrated projects |

### Other

- `Dockerfile.rust`, `Dockerfile.python` — Alpine 3.23 toolchain images for `use-builder` flow.
- `podman.mk` — **deprecated** Make include (feature-parallel to Just modules; uses `::` recipes for extensibility).
- `ecr_lifecycle.py` — boto3 image cleanup for public *and* private ECR. Public/private dispatch and region are derived from the `--registry` URL. `--dev` deletes images whose every tag matches `*-g<hash>` or contains `dirty`.
- `utils.sh` — Bash `bumpVersion` (semver awk) + `addCommitTagPush`.

## Conventions

### Shell escaping in Groovy `sh` calls

Always single-quote interpolated values: `sh "just <target> '${var}'"`. Use `withEnv` for environment variables instead of inline `KEY=VAL ...` so Jenkins sets them directly without shell parsing. Values come from the Jenkinsfile config map (controlled), so single-quote is safe; if a value could contain a `'`, escape it explicitly.

**Optional positional args must be omitted, not passed as `''`.** Just recipes typically default an arg like `image=git_repo_name`; passing an empty string from Groovy overrides that default with empty and breaks the recipe. Pattern:

```groovy
def imageArg = imageName ? " '${imageName}'" : ''
sh "just container::build${imageArg}"
```

### Optional just recipes

Stages call recipes conditionally so consumers don't need to define every target:

```
sh "if just --summary | grep -q lint; then just lint; fi"
```

### Builder container indirection

`needBuilder: true` in the config makes Prepare/Lint/Build run targets inside `local-{toolchain}-builder` via `just use-builder <target>`. The agent only needs Podman/Buildah, not language toolchains.

One working container is created per pipeline (named `local-<toolchain>-builder-<sanitized $BUILD_TAG>`) and reused across stages, so toolchain caches (cargo target, sccache) survive Prepare → Lint → Build → Test. `post.cleanup` calls `container.cleanBuilder(config)` which runs `just clean-builder` to remove it on any outcome. For the rust toolchain only, the sccache cache dir on the agent (`$SCCACHE_DIR` or `~/.cache/sccache`) is bind-mounted into the container, so the cache survives `clean-builder` and is shared with anything else on the agent using sccache.

### Multi-arch

Per-arch images tagged `<image>:<tag>-<arch>`, then `push` builds a manifest list referencing both archs. `all_archs := "amd64 arm64"` in `git.just`.

### Versioning

`git_tag` from `git describe --tags --match "${TAG_MATCH:-v*.*.*}" --dirty`, falls back to short SHA. The default match `v*.*.*` covers single-repo projects; monorepo services override it via `export TAG_MATCH := "<service>/v*.*.*"` in their justfile so `git describe` only considers that service's release tags. On non-main branches, `tag` becomes `<git_tag>-<sanitized-branch>` unless the branch is already a substring/equal.

### Monorepo support

The library is path-aware so a single `.ci/` subtree at the repo root serves multiple services in subdirectories:

- `builder.just` resolves `Dockerfile.<toolchain>` via `source_directory()` so `update-builder` works regardless of caller cwd.
- `use-builder` mounts the repo root (not `$(pwd)`) and `cd`s to the caller's relative path inside the container, so `import '../../.ci/<lang>.just'` from a service justfile resolves at runtime.
- `git.just` honours `$TAG_MATCH` for per-service tag prefixes.
- `container.prepare` defaults `protect` to `["${workDir}/.justfile", "${workDir}/Jenkinsfile", '.ci/**']` — service-scoped without needing per-Jenkinsfile config. Note: protecting `Jenkinsfile` is symbolic only; Jenkins reads it before any pipeline step runs, so `protectBuildFiles` cannot prevent a malicious PR Jenkinsfile from executing. Real defense lives in Jenkins controller config (PR approval policies for external contributors).

Per-service Jenkinsfile typically sets `workDir`, `imageName`, `buildOnly` (regex with the service's path prefix), and `needBuilder`. See README "Monorepo layout" for a full example.

## External integration

- **Gitea** — changeset detection via REST API. Credentials: Jenkins username/password credential ID `gitea-jenkins-password` (default, configurable). API base derived from `env.GIT_URL` or overridden via config.
- **GitOps writeback** — `updateGitops(...)` from a consumer Jenkinsfile commits image tag/digest changes to a Gitea-hosted manifests repo so ArgoCD/Flux pick them up. Two modes: `push` (direct commit to base branch) or `pr` (force-with-lease to a PR branch + idempotent Gitea PR open). Credentials: SSH-key credential when `repo` is `git@host:...` / `ssh://...`, username/password credential when `repo` is `https://...` (uses `gitUsernamePassword` so the token never lands in the URL or `.git/config`). PR mode additionally needs a username/password credential as the Gitea API token. Consumers must `mod gitops '.ci/gitops.just'` so the recipes are reachable. Examples: `examples/Jenkinsfile.gitops-{push,pr}.groovy`.
- **AWS ECR (public or private)** — `aws ecr-public get-login-password` (region `us-east-1`, fixed for ECR Public) or `aws ecr get-login-password` (region parsed from the registry hostname `*.dkr.ecr.<region>.amazonaws.com`) piped to `podman login`. **No default registry** — every consumer must declare it. Jenkins consumers pass `registry: '...'` in `justContainer(...)`; `container.push` validates and forwards it as the first positional arg to the just recipes. Local dev consumers either pass it explicitly or wrap the recipes in their own justfile using a local `registry := "..."` variable. Local dev and Jenkins agent both rely on ambient AWS credentials (env vars, instance profile, etc.) — no credential plumbing in the library.
- **Build notifications (apprise-api)** — optional. `notify.groovy` POSTs build-lifecycle events to an [apprise-api](https://github.com/caronc/apprise-api) sidecar running next to the Jenkins controller, which fans out to chat targets (Slack/Matrix/Mattermost/Teams). Endpoint from `env.APPRISE_API_URL` (controller-global) or `notify.url`. Chat destinations + their secrets live in apprise-api **config keys** (referenced via `notify.key`, defaulting to the `default` key when omitted), not in this repo. Opt-in per consumer via the `notify` config map; uses the existing HTTP Request plugin (no new plugin). See README "Build notifications".
- **Required Jenkins plugins:** Pipeline (declarative), Git, HTTP Request, Pipeline Utility Steps, Credentials Plugin, Warnings Next Generation (`recordIssues` with `grype` and `sarif` tools).

## Known gaps

- **No tests for the library itself.** No Jenkinsfile linter integration. Correctness verified by running against real consumer projects.

## Working with this repo

- Edits to `vars/*.groovy` only affect Jenkins consumers when the library is reloaded (controller-side cache).
- Edits to `*.just` propagate to consumers when they `git subtree pull` the new revision (or run their `ci-pull-upstream` recipe).
- Don't add a recipe to a `*.just` module without considering whether developers will actually run it locally; if it's Jenkins-only, it belongs in Groovy.
- Don't add `sh` logic to Groovy beyond invoking `just`. If the temptation arises, the right move is a new just recipe.
