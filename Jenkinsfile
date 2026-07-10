@Library('ci-tools-lib') _

justContainer(
  registry: 'public.ecr.aws/zero-downtime',
  needBuilder: true,
  buildOnly: ['Cargo.*', 'src\\/.*', 'Dockerfile'],
  notify: [ tag: 'ci' ],
  )
