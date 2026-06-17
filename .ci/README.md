# ci-tools-lib

Various toolchain bits and pieces shared between projects — a shared CI/CD toolchain library for building, testing, scanning, and publishing containerized applications using Podman, Jenkins, and AWS ECR.

## Features

- **Container Build Orchestration** — Podman/Buildah rootless container builds with multi-architecture support (amd64, arm64) and a multi-arch manifest
- **Jenkins Shared Libraries** — Reusable, composable per-stage pipeline templates for Just-based projects
- **Gitea SCM Integration** — Native change detection via API for PR and commit changesets
- **AWS ECR (public or private)** — Registry login, push, manifest management, and automated image lifecycle cleanup; public vs private auto-detected from the registry URL
- **Security Scanning** — Grype vulnerability scanning + betterleaks secret detection (source and image), with configurable severity thresholds and SARIF/JSON reporting surfaced via `recordIssues`
- **Semantic Versioning** — Automatic version computation from git tags with branch suffix support
- **Build Protection** — PR safety mechanism that overwrites build config files from the target branch
- **Builder Containers** — Optional isolated build environments (e.g. Rust toolchain with sccache, cargo-deny, cargo-auditable). One container is reused across all pipeline stages.
- **GitOps Writeback** — Post-build promotion: commit image tag/digest updates to a Gitea-hosted manifests repo (direct push or PR-gated) so ArgoCD/Flux sync the change
- **Build Notifications** — Optional build-lifecycle messages (start / success / failure / aborted) via an apprise-api sidecar, fanning out to Slack / Matrix / Mattermost / Teams

## Quickstart

### 1. Add as a git subtree

```bash
git subtree add --prefix .ci https://git.zero-downtime.net/ZeroDownTime/ci-tools-lib.git main --squash
```

### 2. Configure your project

Import the relevant `.just` modules in your `justfile`:

```just
import '.ci/container.just'
import '.ci/rust.just'
import '.ci/git.just'
```

### 3. Integrate with Jenkins

Add a `Jenkinsfile` using the shared library:

```groovy
@Library('ci-tools-lib') _

justContainer(
  imageName:   'my-app',
  registry:    'public.ecr.aws/<alias>',  // or '<account>.dkr.ecr.<region>.amazonaws.com'
  buildOnly:   ['src/.*', '.justfile'],
  needBuilder: true,
)
```

`registry` is required — set it explicitly per project. The library auto-detects public vs private ECR from the URL shape (`public.ecr.aws/...` vs `*.dkr.ecr.<region>.amazonaws.com`) and dispatches to the correct `aws ecr` / `aws ecr-public` API. Region for private is parsed from the hostname. Both the agent and dev workstation need ambient AWS credentials in scope (env vars, instance profile, etc.) — the library does no credential plumbing.

## Components

### Just — `.just` modules (recommended)

All build logic lives in these modules so a developer reproduces full CI behaviour locally by running the same `just` recipes Jenkins runs.

| Module            | Key Recipes                                              |
|-------------------|----------------------------------------------------------|
| `container.just`  | `build`, `scan` (Grype + betterleaks), `push` (multi-arch manifest), `ecr-login`, `create-repo`, `rm-remote-untagged`, `clean`. Registry-touching recipes take the registry as their first positional arg; public vs private AWS ECR auto-detected from the URL. No default registry — every consumer declares it. |
| `rust.just`       | `prepare` (`cargo fetch --locked`), `lint` (clippy + cargo-deny), `build` (cargo auditable), `test`, `update-lock`, `cut-release`. Opt into an Alpine musl target with `CARGO_BUILD_MUSL`. |
| `python.just`     | uv-based: `prepare` (`uv sync --locked`), `lint` (flake8), `build` (`uv build`), `test` (pytest), `upload` (`uv publish`) |
| `git.just`        | Version computation from tags (`git describe`, `$TAG_MATCH`-aware), branch-suffixed `tag`, `arch` (`$ARCH`, default amd64), `cleanup-tags`, `ci-pull-upstream` |
| `builder.just`    | `update-builder` (build toolchain image), `use-builder <target>` (run a target inside the reused toolchain container; mounts repo root + sccache cache for Rust), `clean-builder` |
| `common.just`     | `scan-src` source secret scan; imported by the language modules |
| `gitops.just`     | `update`. Edits image tags / yq paths in a manifests repo, commits, pushes (with rebase-retry). Commit message comes from `$GITOPS_COMMIT_MESSAGE`. PR opening lives in `gitea.groovy` (`gitea.openPullRequest`). Updates spec is a JSON file so push-mode promotions reproduce locally. |

### Jenkins — Shared Library (`vars/`)

Thin glue only — each helper wraps Jenkins primitives around a `just` invocation; the real logic stays in the `.just` modules.

| Library                  | Purpose                                              |
|--------------------------|------------------------------------------------------|
| `justContainer.groovy`   | Entry point — the declarative pipeline composing the per-stage helpers |
| `container.groovy`       | Per-stage helpers (`changeset`, `prepare`, `lint`, `build`, `test`, `scan`, `push`, `clean`, `cleanBuilder`) invoked by `justContainer` |
| `gitea.groovy`           | Gitea API integration for change detection and PR open/reuse |
| `notify.groovy`          | Optional build-lifecycle notifications via an apprise-api sidecar (see [Build notifications](#build-notifications)) |
| `protectBuildFiles.groovy` | Overwrites CI files from target branch during PR builds |
| `updateGitops.groovy` | GitOps writeback wrapper: commits yq-path updates to a Gitea manifests repo (`push` or `pr` mode). Auto-picks `sshagent` vs. `gitUsernamePassword` from the repo URL scheme. See `examples/Jenkinsfile.gitops-{push,pr}.groovy`. |

**Pipeline stages:** Changeset → Prepare → Lint → Build → Test → Scan → Push → Cleanup

`Changeset` is a minimal first stage that runs the gitea change detection and sets the `SKIP` flag (no changed file matched `buildOnly`, and no force build) — every later stage, `Prepare` included, is gated on it, so the skip decision is made before any prep work runs.

`justContainer` declares a `FORCE_BUILD` boolean build parameter (default off). Tick it in "Build with Parameters" to bypass the `buildOnly` skip gate for a one-off rebuild without editing the Jenkinsfile. (The checkbox appears from the second build onward — Jenkins registers parameters retroactively.)

### Utilities

- **`ecr_lifecycle.py`** — Python utility (requires `boto3`) to manage ECR image lifecycle for public *and* private ECR: removes untagged images, prunes old dev-tagged images, keeps a configurable number of recent tagged images. Detects public vs private from the `--registry` URL.
- **`utils.sh`** — Bash helpers for semantic version bumping (`bumpVersion`) and git commit/tag/push automation (`addCommitTagPush`).
- **`Dockerfile.rust`** — Rust toolchain builder image (Alpine 3.24) with cargo, clippy, sccache (`RUSTC_WRAPPER`), cargo-auditable, cargo-deny, and just. Used by the `use-builder` flow.
- **`Dockerfile.python`** — Python toolchain builder image (Alpine 3.24, uv-based) for the `use-builder` flow.

## Monorepo layout

For a monorepo where each service has its own `.justfile`, `Jenkinsfile`, and `Dockerfile` under a subdirectory (e.g. `services/api-users/`), share one `.ci/` subtree at the repo root and pass per-service config:

```
repo/
├── .ci/                            # git subtree of ci-tools-lib
└── services/
    └── api-users/
        ├── Jenkinsfile
        ├── .justfile
        ├── Dockerfile
        └── pyproject.toml
```

**`services/api-users/.justfile`:**

```just
# Per-service tag prefix so `git describe` only sees this service's releases
export TAG_MATCH := "api-users/v*.*.*"

# Optional (Rust only, needs `needBuilder`): build with an explicit Alpine musl
# target inside the builder, so artifacts land in `target/<triple>/<profile>/`.
# Update this service's Dockerfile `COPY` path to match. The triple follows ARCH
# (x86_64- / aarch64-alpine-linux-musl) and is injected by `use-builder` *inside
# the container only* — plain `just build` on a non-Alpine dev host is unaffected.
# Do NOT `export CARGO_BUILD_TARGET` here: it would also hit host cargo and break
# local `just build` off Alpine.
# export CARGO_BUILD_MUSL := "true"

# Toolchain — flat-imported so `just lint`, `just prepare`, `just scan-src`,
# `just use-builder lint` etc. work. Pulls common.just, builder.just, git.just.
import '../../.ci/python.just'

# Container recipes namespaced — Jenkins glue calls `just container::build`.
mod container '../../.ci/container.just'
```

**`services/api-users/Jenkinsfile`:**

```groovy
@Library('ci-tools-lib') _

justContainer(
    workDir:     'services/api-users',
    imageName:   'api-users',
    registry:    '1234567890.dkr.ecr.us-east-1.amazonaws.com',  // or public.ecr.aws/<alias>
    buildOnly:   ['services/api-users/.*', '\\.ci/.*'],
    needBuilder: true,
    // Extra env (withEnv format) applied to the Prepare/Lint/Build/Test stages.
    // Static values only. Host-safe Rust opt-ins (read by use-builder inside the
    // Alpine builder): env: ['CARGO_BUILD_MUSL=true', 'ARCH=arm64'].
    // env: ['CARGO_BUILD_MUSL=true'],
    // Optional build notifications via the apprise-api sidecar (see below).
    // notify: [key: 'team-platform'],   // events default to start/success/failure/aborted
)
```

`protect` defaults to `["${workDir}/.justfile", "${workDir}/Jenkinsfile", '.ci/**']`, so service-scoped build files are restored from the target branch on PR builds without needing to override it. Tag releases as `api-users/v1.2.3` and configure the Jenkins multibranch project's *Script Path* to `services/*/Jenkinsfile`.

## Build notifications

Optional build-lifecycle notifications (start / success / failure / aborted) are sent through an [apprise-api](https://github.com/caronc/apprise-api) sidecar running next to the Jenkins controller. `justContainer` POSTs a single JSON event; apprise-api fans out to the configured chat targets (Slack, Matrix, Mattermost, Teams, ...). Off unless `notify` is set — existing consumers are unaffected.

Destinations and their secrets live in apprise-api **config keys**, never in this repo: register e.g. `slack://…`/`matrix://…` URLs under a key (`team-platform`) on the sidecar, then reference the key from the Jenkinsfile. If `key` is omitted (and no `urls` are given), the module falls back to apprise-api's conventional `default` key — so a single shared `default` config requires no per-Jenkinsfile `key` at all.

```groovy
justContainer(
    // ...
    notify: [
        // key:        'team-platform',                // apprise-api config key (defaults to 'default' when omitted)
        // urls:       ['slack://T/B/xxx'],             // stateless alternative (destinations in-repo)
        // events:     ['start', 'success', 'failure', 'aborted'],  // this is the default set
        tag:           'ci',                            // optional apprise tag filter
        // url:        'http://apprise-api:8000',       // optional; defaults to env.APPRISE_API_URL
        // credentialsId: 'apprise-token',              // optional Secret Text -> 'Authorization: Bearer <token>'
        // notifySkipped: true,                         // also notify SKIP (no-change) builds
        // messages:   [failure: { "build broke: ${env.BUILD_URL}" }],  // optional per-event body closures
    ],
)
```

- **Endpoint** is shared infra, so set `APPRISE_API_URL` once on the controller (global env); `notify.url` overrides per-job.
- **Destinations** resolve in this order: an explicit `key` → `/notify/<key>`; else inline `urls` → stateless `/notify`; else the `default` key → `/notify/default`. Pre-register that `default` config on the sidecar to drive notifications from `notify: [events: [...]]` alone.
- **Events** map from the build result: `SUCCESS→success`, `FAILURE→failure`, `UNSTABLE→unstable`, `ABORTED`/`NOT_BUILT→aborted`, plus `start`. All five fire by default (`['start','success','failure','aborted']`); set `events` to narrow the set. The apprise notification `type` (`info`/`success`/`warning`/`failure`) drives per-platform colour automatically.
- **Aborted builds** fire from `post { always }` like any other end event. A UI abort interrupts the in-flight notification step once, so the send is retried a single time to survive the abort — a hard/double-kill that tears the executor down may still skip it.
- **SKIP builds** (no source changes) are silent unless `notifySkipped: true` (governs start and end symmetrically).
- **Title** reads like a sentence with a leading status emoji and, on end events, a build-status transition vs. the previous run — e.g. `🚀 Jenkins build of api-users/main started`, `✅ Jenkins build of api-users/main finished successfully (Fixed)`, `❌ … failed (Still failing)`.
- **Body** defaults to a one-line summary: a PR/branch ref (PR builds link to `CHANGE_URL`; branch builds link to the gitea branch page), the short commit SHA (linked to the gitea commit page), and a linked Jenkins `build <number>` followed by the trigger cause (`triggered by user …`) on start events, or the duration (`took …`) on end events. All git links are derived in-process from `GIT_URL` (via `gitea.parseGitUrl`) — no remote call. Override any event's body with a closure under `messages` (resolved lazily so `env`/`currentBuild` are populated).
- A notification problem (sidecar down, bad key) is logged and **never fails the build**.

## GitOps writeback

Promote a freshly built image into a Gitea-hosted manifests repo so ArgoCD/Flux picks it up. The image tag is captured from `container.push(config)`'s return value (the actual `git_tag` published — e.g. `v1.2.3` on a tagged commit) and threaded into the Promote stage:

```groovy
@Library('ci-tools-lib') _

def config   = [imageName: 'payments', registry: '...', /* ... */]
def imageTag                                   // captured in Push, consumed in Promote

pipeline {
    // ... agent, Prepare/Lint/Build/Test/Scan stages calling container.<stage>(config) ...

    stage('Push')   { steps { script { imageTag = container.push(config) } } }

    stage('Promote') {
        steps { script {
            updateGitops(
                repo:          'git@git.zero-downtime.net:zdt/infra.git',  // or https://...
                branch:        'main',
                credentialsId: 'infra-repo-deploy-key',                    // SSH key, or userpass for HTTPS
                updates: [
                    'apps/payments/values.yaml': [
                        '.image.tag': imageTag,
                    ],
                ],
            )
        } }
    }
}
```

PR-gated mode adds `mode: 'pr'`, `tokenCredentialsId:` (Gitea API token), `prBranch:`, `prTitle:`, `prBody:`. Returns `[sha, branch, prUrl]`. The PR branch is reused on re-runs (idempotent: existing open PR URL is returned). See `examples/Jenkinsfile.gitops-push.groovy` and `examples/Jenkinsfile.gitops-pr.groovy` for full pipelines.

Reproduce locally with the same recipes Jenkins runs:

```bash
echo '{"apps/payments/values.yaml":{".image.tag":"v1.2.3"}}' > /tmp/u.json
just gitops::update git@git.zero-downtime.net:zdt/infra.git main /tmp/u.json
```

The consumer's root justfile must import the module: `mod gitops '.ci/gitops.just'`.

## Local dev

Recipes that touch the registry take it as their first positional argument:

```bash
just container::build my-app                                          # registry not needed
just container::ecr-login public.ecr.aws/<alias>
just container::push public.ecr.aws/<alias> my-app
just container::create-repo public.ecr.aws/<alias> my-app
```

For ergonomics, define the registry once in your project's root `.justfile` and add convenience wrappers:

```just
registry := "public.ecr.aws/<alias>"          # or "<account>.dkr.ecr.<region>.amazonaws.com"

mod container '.ci/container.just'
import '.ci/python.just'                       # or rust.just

# Convenience wrappers — pass the registry through to module recipes
push image="":
  just container::push {{ registry }} {{ image }}

ecr-login:
  just container::ecr-login {{ registry }}

create-repo image="":
  just container::create-repo {{ registry }} {{ image }}
```

`build`, `scan`, and `clean` recipes don't take a registry, so they remain reachable as `just container::build` etc. without any wrapping. The Jenkins glue passes the registry directly from the `registry:` config field — consumers don't need wrappers for CI.

## Maintenance

Pull the latest upstream changes into your project:

```bash
git subtree pull --prefix .ci https://git.zero-downtime.net/ZeroDownTime/ci-tools-lib.git main --squash
```

## Renovate

Run renovate locally to test custom config:

```bash
LOG_LEVEL=debug ~/node_modules/renovate/dist/renovate.js --platform local --dry-run
```

## License

[GNU AGPL v3](LICENSE)
