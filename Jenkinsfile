@Library('zdt-ci-tools') _

runPipeline(
  [
    registry: 'public.ecr.aws/zero-downtime',
    needBuilder: true,
    buildOnly: ['Cargo.*', 'src\\/.*', 'Dockerfile'],
    notify: [ tag: 'ci' ],
  ],
  stagesLibrary.defaults(),
)
