library identifier: 'zdt-lib@main', retriever: modernSCM(
  [$class: 'GitSCMSource',
   remote: 'https://git.zero-downtime.net/ZeroDownTime/ci-tools-lib.git'])

buildPodman name: 'redis-vault', buildOnly: ['Cargo.*', 'src\\/.*', 'Dockerfile']
