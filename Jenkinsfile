library identifier: 'zdt-lib@main', retriever: modernSCM(
  [$class: 'GitSCMSource',
   remote: 'https://git.zero-downtime.net/ZeroDownTime/ci-tools-lib.git'])

protectBuildFiles(['Makefile', '.justfile', '.ci/**'])

justContainer buildOnly: ['Cargo.*', 'src\\/.*', 'Dockerfile']
