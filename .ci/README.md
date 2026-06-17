# ci-tools-lib

Various toolchain bits and pieces shared between projects — a shared CI/CD toolchain library for building, testing, scanning, and publishing containerized applications using Podman, Jenkins, and AWS ECR.

## Features

- **Container Build Orchestration** — Podman-based rootless container builds with multi-architecture support (amd64, arm64)
- **Jenkins Shared Libraries** — Reusable pipeline templates for Just and Make-based projects
- **Gitea SCM Integration** — Native change detection via API for PR and commit changesets
- **AWS ECR Public** — Registry login, push, manifest management, and automated image lifecycle cleanup
- **Vulnerability Scanning** — Grype integration with configurable severity thresholds and JSON reporting
- **Semantic Versioning** — Automatic version computation from git tags with branch suffix support
- **Build Protection** — PR safety mechanism that overwrites build config files from the target branch
- **Builder Containers** — Optional isolated build environments (e.g. Rust toolchain with sccache, cargo-deny, cargo-auditable). One container is reused across all pipeline stages.
- **GitOps Writeback** — Post-build promotion: commit image tag/digest updates to a Gitea-hosted manifests repo (direct push or PR-gated) so ArgoCD/Flux sync the change

## Quickstart

### 1. Add as a git subtree

```bash
git subtree add --prefix .ci https://git.zero-downtime.net/ZeroDownTime/ci-tools-lib.git main --squash
```

### 2. Configure your project

**Using Just** (recommended) — Import the relevant `.just` modules in your `justfile`:

```just
import '.ci/container.just'
import '.ci/rust.just'
import '.ci/git.just'
```

**Using Make** (deprecated — support will be removed midterm) — Create a top-level `Makefile`:

```makefile
REGISTRY := public.ecr.aws/<alias>             # or 1234567890.dkr.ecr.<region>.amazonaws.com
IMAGE := <image_name>

include .ci/podman.mk
```

### 3. Integrate with Jenkins

Add a `Jenkinsfile` using the shared libraries:

```groovy
@Library('ci-tools-lib') _

// Just-based projects (recommended)
justContainer(
  imageName:   'my-app',
  registry:    'public.ecr.aws/<alias>',  // or '<account>.dkr.ecr.<region>.amazonaws.com'
  buildOnly:   ['src/.*', '.justfile'],
  needBuilder: true,
)

// Or Make-based projects (deprecated)
buildPodman(
  buildOnly: ['src/.*', 'Cargo.*'],
)
```

`registry` is required — set it explicitly per project. The library auto-detects public vs private ECR from the URL shape (`public.ecr.aws/...` vs `*.dkr.ecr.<region>.amazonaws.com`) and dispatches to the correct `aws ecr` / `aws ecr-public` API. Region for private is parsed from the hostname. Both the agent and dev workstation need ambient AWS credentials in scope (env vars, instance profile, etc.) — the library does no credential plumbing.

## Components

### Just — `.just` modules (recommended)

| Module            | Key Recipes                                              |
|-------------------|----------------------------------------------------------|
| `container.just`  | `build`, `scan`, `push`, `ecr-login`, `create-repo`, `clean`, manifest management. Public and private AWS ECR auto-detected; override default via `REGISTRY` env var. |
| `rust.just`       | `prepare`, `lint` (clippy + cargo-deny), `build`, `test`, version bumping |
| `git.just`        | Version computation from tags, `tag-push`, legacy tag cleanup |
| `builder.just`    | Builder container creation and execution via Buildah      |
| `common.just`     | `scan-src` source secret scan; imported by language modules |
| `gitops.just`     | `update`. Edits image tags / yq paths in a manifests repo, commits, pushes (with rebase-retry). Commit message comes from `$GITOPS_COMMIT_MESSAGE`. PR opening lives in `gitea.groovy` (`gitea.openPullRequest`). Updates spec is a JSON file so push-mode promotions reproduce locally. |

### Make — `podman.mk` (deprecated — support will be removed midterm)

Common Makefile include providing standardized build targets:

| Target                | Description                          |
|-----------------------|--------------------------------------|
| `make help`           | Show available targets               |
| `make prepare`        | Custom pre-build preparation         |
| `make fmt`            | Auto-format source code              |
| `make lint`           | Lint source code                     |
| `make build`          | Build container image                |
| `make test`           | Test built artifacts                 |
| `make scan`           | Scan image with Grype                |
| `make push`           | Push image to registry               |
| `make ecr-login`      | Login to AWS ECR                     |
| `make rm-remote-untagged` | Cleanup untagged/dev images     |
| `make create-repo`    | Create AWS ECR (public or private) repository |
| `make clean`          | Clean up build artifacts             |
| `make ci-pull-upstream` | Pull latest `.ci` subtree          |

### Jenkins — Shared Libraries (`vars/`)

| Library                  | Purpose                                              |
|--------------------------|------------------------------------------------------|
| `justContainer.groovy`   | Full pipeline for Just-based container projects       |
| `buildPodman.groovy`     | Full pipeline for Make-based container projects (deprecated) |
| `gitea.groovy`           | Gitea API integration for change detection            |
| `protectBuildFiles.groovy` | Overwrites CI files from target branch during PR builds |
| `updateGitops.groovy` | GitOps writeback wrapper: commits yq-path updates to a Gitea manifests repo (`push` or `pr` mode). Auto-picks `sshagent` vs. `gitUsernamePassword` from the repo URL scheme. See `examples/Jenkinsfile.gitops-{push,pr}.groovy`. |

**Pipeline stages:** Changeset → Prepare → Lint → Build → Test → Scan → Push → Cleanup

`Changeset` is a minimal first stage that runs the gitea change detection and sets the `SKIP` flag (no changed file matched `buildOnly`, and no force build) — every later stage, `Prepare` included, is gated on it, so the skip decision is made before any prep work runs.

`justContainer` declares a `FORCE_BUILD` boolean build parameter (default off). Tick it in "Build with Parameters" to bypass the `buildOnly` skip gate for a one-off rebuild without editing the Jenkinsfile. (The checkbox appears from the second build onward — Jenkins registers parameters retroactively.)

### Utilities

- **`ecr_lifecycle.py`** — Python utility (requires `boto3`) to manage ECR image lifecycle for public *and* private ECR: removes untagged images, prunes old dev-tagged images, keeps a configurable number of recent tagged images. Detects public vs private from the `--registry` URL.
- **`utils.sh`** — Bash helpers for semantic version bumping (`bumpVersion`) and git commit/tag/push automation (`addCommitTagPush`).
- **`Dockerfile.rust`** — Multi-stage Rust builder image (Alpine 3.23) with cargo, clippy, sccache, cargo-auditable, cargo-deny, and just.

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
    // notify: [key: 'team-platform', events: ['start', 'success', 'failure']],
)
```

`protect` defaults to `["${workDir}/.justfile", "${workDir}/Jenkinsfile", '.ci/**']`, so service-scoped build files are restored from the target branch on PR builds without needing to override it. Tag releases as `api-users/v1.2.3` and configure the Jenkins multibranch project's *Script Path* to `services/*/Jenkinsfile`.

## Build notifications

Optional build-lifecycle notifications (start / success / failure) are sent through an [apprise-api](https://github.com/caronc/apprise-api) sidecar running next to the Jenkins controller. `justContainer` POSTs a single JSON event; apprise-api fans out to the configured chat targets (Slack, Matrix, Mattermost, Teams, ...). Off unless `notify` is set — existing consumers are unaffected.

Destinations and their secrets live in apprise-api **config keys**, never in this repo: register e.g. `slack://…`/`matrix://…` URLs under a key (`team-platform`) on the sidecar, then reference the key from the Jenkinsfile. If `key` is omitted (and no `urls` are given), the module falls back to apprise-api's conventional `default` key — so a single shared `default` config requires no per-Jenkinsfile `key` at all.

```groovy
justContainer(
    // ...
    notify: [
        // key:        'team-platform',                // apprise-api config key (defaults to 'default' when omitted)
        // urls:       ['slack://T/B/xxx'],             // stateless alternative (destinations in-repo)
        events:        ['start', 'success', 'failure', 'aborted'], // default ['success', 'failure']
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
- **Events** map from the build result: `SUCCESS→success`, `FAILURE→failure`, `UNSTABLE→unstable`, `ABORTED→aborted`, plus `start`. Only listed events fire — to be notified when a build is aborted (incl. via the Jenkins UI), add `'aborted'` to `events`; it is not in the default set. The apprise notification `type` (`info`/`success`/`warning`/`failure`) drives per-platform colour/emoji automatically.
- **Aborted builds** fire from `post { always }` like any other end event. A UI abort interrupts the in-flight notification step once, so the send is retried a single time to survive the abort — a hard/double-kill that tears the executor down may still skip it.
- **SKIP builds** (no source changes) are silent unless `notifySkipped: true`.
- **Messages** default to a one-line summary: a PR/branch ref (PR builds link to `CHANGE_URL`; branch builds link to the gitea branch page), the short commit SHA (linked to the gitea commit page), and a linked Jenkins `build <number>` followed by the trigger cause (`triggered by user …`) on start events, or the duration (`took …`) on end events. All links are derived in-process from `GIT_URL` — no remote call. Override any event's body with a closure under `messages` (resolved lazily so `env`/`currentBuild` are populated).
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
