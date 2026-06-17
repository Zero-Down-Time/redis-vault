/**
 * Build lifecycle notifications via an apprise-api sidecar.
 *
 * Pure Jenkins glue: the pipeline POSTs a single JSON event to apprise-api,
 * which fans out to the configured chat targets (Slack, Matrix, Mattermost,
 * Teams, ...). Destinations + secrets live in apprise-api config *keys*, not in
 * this repo. Notifying a chat channel is a CI-lifecycle concern with no local
 * `just` equivalent, so it correctly lives in Groovy rather than a *.just module.
 *
 * required plugins:
 * - HTTP Request
 * - Pipeline Utility Steps (writeJSON)
 * - Credentials Plugin (only when notify.credentialsId is set)
 *
 * Opt in from a consumer Jenkinsfile:
 *
 *   justContainer(
 *     ...,
 *     notify: [
 *       key:    'team-platform',                  // apprise-api config key (recommended)
 *       events: ['start', 'success', 'failure'],  // default ['success', 'failure']
 *     ],
 *   )
 *
 * The apprise-api endpoint defaults to env.APPRISE_API_URL (set once on the
 * controller, since the sidecar is shared infra) and is overridable per-job via
 * notify.url.
 */

// Fire a "build started" event. Safe to call unconditionally
def start(Map config = [:]) {
    _emit(config, 'start', 'info')
}

// Fire a "build ended" event. Derives the event + apprise type from
// currentBuild.currentResult, so this is safe to call unconditionally from
// post { always { } } — it self-filters by result, events and the SKIP flag.
def end(Map config = [:]) {
    def result = currentBuild.currentResult ?: 'SUCCESS'
    def map = [
        SUCCESS:   [event: 'success',  type: 'success'],
        FAILURE:   [event: 'failure',  type: 'failure'],
        UNSTABLE:  [event: 'unstable', type: 'warning'],
        ABORTED:   [event: 'aborted',  type: 'warning'],
        NOT_BUILT: [event: 'aborted',  type: 'warning'],
    ][result] ?: [event: 'failure', type: 'failure']

    _emit(config, map.event, map.type)
}

// Resolve config, apply the event/SKIP filters, build the message and send.
// Never throws: a notification problem must not fail the build.
def _emit(Map config, String event, String type) {
    try {
        def n = config.notify
        if (!(n instanceof Map)) return

        def events = n.events ?: ['start', 'success', 'failure', 'aborted']
        if (!events.contains(event)) return

        def skipped = currentBuild.description == 'SKIP'
        if (skipped && !(n.notifySkipped ?: false)) return

        def apiUrl = (n.url ?: env.APPRISE_API_URL)?.toString()?.replaceAll('/$', '')
        if (!apiUrl) {
            echo "notify: no apprise-api url (set env.APPRISE_API_URL or notify.url); skipping ${event}"
            return
        }

        def debug = n.debug ?: false
        def payload = [
            title: _title(event),
            body:  _body(n, event),
            type:  type,
            format: 'markdown',
        ]
        if (n.tag)  payload.tag  = n.tag
        if (n.urls) payload.urls = n.urls instanceof List ? n.urls.join(',') : n.urls

        // Keyed (stateful) mode resolves destinations server-side from the apprise-api
        // config stored under `key`; absent an explicit key, fall back to apprise-api's
        // conventional 'default' key. A consumer that sets `urls` instead opts into
        // stateless mode (`/notify`, inline urls — no stored config).
        def endpoint = n.key  ? "${apiUrl}/notify/${n.key}"
                     : n.urls ? "${apiUrl}/notify"
                     :          "${apiUrl}/notify/default"
        _dispatch(n, endpoint, payload, debug)
    } catch (err) {
        echo "notify: ${event} notification failed (ignored): ${err}"
    }
}

// Send once, retrying a single time on FlowInterruptedException. A build aborted
// via the UI delivers that interrupt to the first interruptible step that runs
// inside post {} — for us the credential binding / httpRequest — so the only POST
// attempt is killed before the event leaves the agent and the abort goes silent.
// The interrupt is one-shot: once it has propagated out of the step it is consumed,
// so a single retry delivers the event. A second failure falls through to _emit's
// catch (logged, never rethrown — the build is already ABORTED).
def _dispatch(Map n, String endpoint, Map payload, Boolean debug) {
    try {
        _sendOnce(n, endpoint, payload, debug)
    } catch (org.jenkinsci.plugins.workflow.steps.FlowInterruptedException ignored) {
        _sendOnce(n, endpoint, payload, debug)
    }
}

def _sendOnce(Map n, String endpoint, Map payload, Boolean debug) {
    if (n.credentialsId) {
        withCredentials([string(credentialsId: n.credentialsId, variable: 'APPRISE_TOKEN')]) {
            _post(endpoint, payload, [[name: 'Authorization', value: "Bearer ${env.APPRISE_TOKEN}", maskValue: true]], debug)
        }
    } else {
        _post(endpoint, payload, [], debug)
    }
}

def _post(String endpoint, Map payload, List headers, Boolean debug) {
    def resp = httpRequest(
        url: endpoint,
        httpMode: 'POST',
        contentType: 'APPLICATION_JSON',
        customHeaders: headers,
        requestBody: writeJSON(returnText: true, json: payload),
        validResponseCodes: '100:599',
        quiet: !debug,
    )
    if ((resp.status as int) >= 300) {
        echo "notify: apprise-api returned HTTP ${resp.status}: ${resp.content}"
    }
}

// Default title, e.g. "Jenkins build of api-users/main finished successfully (Fixed)"
// or "Jenkins build of api-users/main started". End events append the build-status
// transition (vs. the previous build) when it carries signal, derived from
// currentBuild only (no remote call). The trigger cause lives in the body.
def _title(String event) {
    def phrase = [
        start:    'started',
        success:  'finished successfully',
        failure:  'failed',
        unstable: 'finished unstable',
        aborted:  'was aborted',
    ][event] ?: event
    def detail = (event == 'start') ? null : _transition()
    def title = "Jenkins build of ${_project()} ${phrase}"
    if (detail) title += " (${detail})"
    return title
}

// "<project>/<branch>" identifying the build. On a multibranch job JOB_BASE_NAME
// is only the branch and JOB_NAME's branch segment is URL-encoded (feature%2Ffoo),
// so take the project (parent) segment from JOB_NAME and the decoded BRANCH_NAME.
// Falls back to the plain JOB_NAME for non-multibranch jobs.
def _project() {
    def full = env.JOB_NAME ?: env.JOB_BASE_NAME ?: 'build'
    if (env.BRANCH_NAME) {
        def segs = full.split('/')
        def project = segs.length >= 2 ? segs[-2] : segs[-1]
        return "${project}/${env.BRANCH_NAME}"
    }
    return full
}

// Human-readable trigger cause for this run (e.g. "Started by user Stefan",
// "Started by timer", "Started by an SCM change"), from currentBuild.buildCauses.
// Returns null if unavailable. Guarded so it can never sink the notification.
def _triggerCause() {
    try {
        def causes = currentBuild.buildCauses
        return causes ? causes[0].shortDescription : null
    } catch (ignored) {
        return null
    }
}

// Build-status transition relative to the previous run, from currentBuild only
// (no remote call): null for a steady-green build (no extra signal), else a
// short label — "Fixed", "Broken", "Still failing", "First build", etc.
def _transition() {
    def prev = currentBuild.previousBuild?.result
    def cur  = currentBuild.currentResult ?: 'SUCCESS'
    if (!prev)        return 'First build'
    if (prev == cur)  return cur == 'SUCCESS' ? null : "Still ${cur.toLowerCase()}"
    if (cur == 'SUCCESS')  return 'Fixed'
    if (prev == 'SUCCESS') return 'Broken'
    return "Now ${cur.toLowerCase()}"
}

// Default body, or a consumer-supplied closure under notify.messages[event].
// Closures are resolved here (lazily) so env/currentBuild are populated.
def _body(Map n, String event) {
    def override = (n.messages instanceof Map) ? n.messages[event] : null
    if (override instanceof Closure) return override.call().toString()
    if (override != null) return override.toString()

    def parts = []
    if (env.CHANGE_ID) {
        // PR build: env.CHANGE_URL is the PR's web URL (set by the multibranch
        // source) — no construction or remote call needed.
        parts << (env.CHANGE_URL ? "<${env.CHANGE_URL}|PR #${env.CHANGE_ID}>" : "PR #${env.CHANGE_ID}")
    } else {
        def branch = (env.BRANCH_NAME ?: env.GIT_BRANCH)?.replaceFirst(/^origin\//, '')
        if (branch) {
            def branchUrl = _branchUrl(branch)
            parts << (branchUrl ? "<${branchUrl}|${branch}>" : branch)
        }
    }
    if (env.GIT_COMMIT) {
        def shortSha = env.GIT_COMMIT.take(8)
        def commitUrl = _commitUrl()
        parts << (commitUrl ? "<${commitUrl}|${shortSha}>" : shortSha)
    }

    // Build segment: linked "build <n>". Start events append the trigger cause
    // ("Started by " rephrased to "triggered by"); end events append the duration.
    def buildSeg = env.BUILD_URL ? "<${env.BUILD_URL}|build ${env.BUILD_NUMBER}>" : "build ${env.BUILD_NUMBER}"
    if (event == 'start') {
        def cause = _triggerCause()?.replaceFirst(/^Started by /, '')
        if (cause) buildSeg += " triggered by ${cause}"
    } else {
        buildSeg += " took ${currentBuild.durationString?.replace(' and counting', '') ?: 'n/a'}"
    }
    parts << buildSeg

    return parts.join(' · ')
}

// "${giteaUrl}/${owner}/${repo}" for the build's repo, derived in-process from
// env.GIT_URL via gitea.parseGitUrl (no remote call). Returns null when GIT_URL
// is absent or unparseable. Wrapped so a link-building hiccup can never sink the
// whole notification.
def _repoBase() {
    try {
        if (!env.GIT_URL) return null
        def g = gitea.parseGitUrl(env.GIT_URL)
        return g ? "${g.giteaUrl}/${g.owner}/${g.repo}" : null
    } catch (ignored) {
        return null
    }
}

// Gitea web URL for the current commit; null when unavailable (caller falls back
// to a plain short SHA).
def _commitUrl() {
    def base = _repoBase()
    return (base && env.GIT_COMMIT) ? "${base}/commit/${env.GIT_COMMIT}" : null
}

// Gitea web URL for a branch; null when unavailable (caller falls back to plain
// branch text).
def _branchUrl(String branch) {
    def base = _repoBase()
    return (base && branch) ? "${base}/src/branch/${branch}" : null
}

return this
