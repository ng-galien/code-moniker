import json
import sys


def run():
	return json.dumps({"limit": sys.getrecursionlimit()})
