# bumps git tag using semVer
bump-version() {
  V=$1
	type=${2:-patch}

	if [ "$type" == 'patch' ]; then
		awkV='{OFS="."; $NF+=1; print $0}'
	elif [ "$type" == 'minor' ]; then
		awkV='{OFS="."; $2+=1; $3=0; print $0}'
	elif [ "$type" == 'major' ]; then
		awkV='{OFS="."; $1+=1; $2=0; $3=0; print "v"$0}'
	else
		echo 'No version type specified.  Specify one of patch, minor, or major.'
		exit 1
	fi

	echo $V | awk -F. "$awkV"
}
