#!/bin/sh

case "${CODE_MONIKER_FAKE_GIT_MODE:-incompatible}" in
	hang)
		sleep 30
		;;
	malformed)
		printf '%s\n' 'not git'
		;;
	incompatible)
		printf '%s\n' 'git version 2.21.0'
		;;
	*)
		printf '%s\n' 'git version 2.47.1.windows.1'
		;;
esac
