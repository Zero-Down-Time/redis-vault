// just based container builder

def call(Map config=[:]) {
    def buildOnly = config.buildOnly ?: ['.*']
    def debug = config.debug ?: false
    def force_build = config.force_build ?: false
    def needBuilder = config.needBuilder ?: false
    def imageName = config.imageName ?: ""
    def scanFail = config.scanFail ?: true

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

            // Overwrite build files from the target/origin branch
            protectBuildFiles(['.justfile', '.ci/**'])

            script {
              // build reports
              sh "mkdir -p reports"

              // Build project specific builder
              if (needBuilder) {
                sh "just update-builder"
              }
            }
          }
        }

        stage('Lint') {
          steps {
            script {
              // Scan for secrets first thing
              sh "betterleaks dir . --validation false --report-path reports/betterleaks-src-report.json --report-format sarif"

              if (needBuilder) {
                sh "just use-builder lint"
              } else {
                sh "just lint"
              }
            }
          }
        }

        // Build using rootless podman
        stage('Build') {
          steps {
            script {
              unstash 'changeSet'
              def files = readJSON file: "changeSet.json"

              if (force_build || gitea.pathsChanged(files: files, patterns: buildOnly, debug: debug)) {
                if (needBuilder) {
                  sh "just use-builder build release"
                }
                sh "just container::build ${imageName}"
              } else {
                echo("No changed files matching any of: ${buildOnly.join(', ')}. No build required.")
                currentBuild.description = 'SKIP'
              }
            }
          }
        }

        stage('Test') {
          when {
            expression { currentBuild.description != 'SKIP' }
          }
          steps {
            sh "echo"
            // sh "just container::test"
          }
        }

        // Scan using grype
        stage('Scan') {
          when {
            expression { currentBuild.description != 'SKIP' }
          }
          steps {
            // we always scan and create the full json report
            sh "GRYPE_OUTPUT=json GRYPE_FILE='reports/grype-report.json' just container::scan ${imageName}"
          }
        }

        // Push to container registry if not PR
        // incl. basic registry retention removing any untagged images
        stage('Push') {
          when {
            expression { currentBuild.description != 'SKIP' }
            not { changeRequest() }
          }
          steps {
            sh "just container::push ${imageName}"
            sh "just container::rm-remote-untagged ${imageName}"
          }
        }

        // generic clean
        stage('cleanup') {
          steps {
            sh "just container::clean ${imageName}"
          }
        }
      }

      post {
        always {
          recordIssues (
            enabledForFailure: true, sourceCodeRetention: 'NEVER', skipPublishingChecks: true,
            qualityGates: [[threshold: 1, type: 'TOTAL_ERROR', criticality: scanFail ? 'CRITICAL' : 'NOTE']],
            tools: [
              grype(),
              sarif(pattern: 'reports/betterleaks*.json')
            ]
          )
        }
        cleanup {
          sh "rm -rf reports"
        }
      }
    }
}
