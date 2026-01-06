// Common container builder by ZeroDownTime

def call(Map config=[:]) {
    def buildOnlyChangeSets = config.buildOnlyChangeSets ?: ['/.*/']
    def debug = config.debug ?: false

    pipeline {
      options {
        disableConcurrentBuilds()
      }
      agent {
        node {
          label 'podman-aws-grype'
        }
      }
      stages {
        stage('Prepare') {
          steps {
            // create and stash changeSet
            script {
              def files = gitea.getChangeset(debug: debug)
              writeJSON file: 'changeSet.json', json: files
              stash includes: 'changeSet.json', name: 'changeSet'
            }

            // Optional project specific preparations
            sh 'mkdir -p reports'
            sh 'make prepare'
          }
        }

        stage('Lint') {
          steps {
            sh 'make lint'
          }
        }

        // Build using rootless podman
        stage('Build') {
          steps {
            script {
              unstash 'changeSet'
              def files = readJSON file: "changeSet.json"

              if (gitea.pathsChanged(files: files, patterns: buildOnlyChangeSets)) {
                sh 'make build GIT_BRANCH=$GIT_BRANCH'
              } else {
                currentBuild.result = 'ABORTED'
                error("No changed files matching ${patterns.join(', ')}. No build required.")
              }
            }
          }
        }

        stage('Test') {
          steps {
            sh 'make test'
          }
        }

        // Scan using grype
        stage('Scan') {
          steps {
            // we always scan and create the full json report
            sh 'GRYPE_OUTPUT=json GRYPE_FILE="reports/grype-report.json" make scan'

            // fail build if grypeFail is set, default is any ERROR marks build unstable
            script {
              def failBuild=config.grypeFail
              if (failBuild == null || failBuild.isEmpty()) {
                  recordIssues enabledForFailure: true, tool: grype(), sourceCodeRetention: 'NEVER', skipPublishingChecks: true, qualityGates: [[threshold: 1, type: 'TOTAL_ERROR', criticality: 'NOTE']]
              } else {
                  recordIssues enabledForFailure: true, tool: grype(), sourceCodeRetention: 'NEVER', skipPublishingChecks: true, qualityGates: [[threshold: 1, type: 'TOTAL_ERROR', criticality: 'FAILURE']]
              }
            }
          }
        }

        // Push to container registry if not PR
        // incl. basic registry retention removing any untagged images
        stage('Push') {
          when { not { changeRequest() } }
          steps {
            sh 'make push'
            sh 'make rm-remote-untagged'
          }
        }

        // generic clean
        stage('cleanup') {
          steps {
            sh 'make clean'
          }
        }
      }
    }
}
