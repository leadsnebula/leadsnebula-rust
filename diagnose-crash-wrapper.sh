#!/bin/bash
# Wrapper to run diagnose-crash.sh explicitly with bash
exec bash "$(dirname "$0")/diagnose-crash.sh" "$@"
