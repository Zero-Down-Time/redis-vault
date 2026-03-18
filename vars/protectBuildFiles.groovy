// Overwrite build files from the target/origin branch
def call(List files = ['Makefile', '.justfile']) {
    sh "git fetch origin ${env.CHANGE_TARGET}"
    sh "git checkout origin/${env.CHANGE_TARGET} -- ${files.join(' ')}"
}
