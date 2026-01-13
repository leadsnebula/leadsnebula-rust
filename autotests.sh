#!/usr/bin/env bash
# autotests.sh - run repository auto-tests and produce a summary report
set -u

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

LOGDIR="$ROOT_DIR/autotest-logs"
mkdir -p "$LOGDIR"

declare -A status

run_step() {
  name="$1"
  shift
  logfile="$LOGDIR/${name// /_}.log"
  echo "==> Running: $name"
  echo "Command: $*" | tee "$logfile"
  # Run command, tee both stdout and stderr
  (set -o pipefail; "$@") 2>&1 | tee -a "$logfile"
  rc=${PIPESTATUS[0]:-0}
  status["$name"]=$rc
  if [ $rc -eq 0 ]; then
    echo "--> $name: PASS"
  else
    echo "--> $name: FAIL (exit $rc)"
  fi
  echo
  return $rc
}

echo "autotests.sh: logs -> $LOGDIR"

# 1) Format check
run_step "fmt-check" cargo fmt --all -- --check || true

# 2) Lint (clippy)
run_step "clippy" bash -lc 'cargo clippy --all-targets --all-features -- -D warnings' || true

# 3) Ensure cargo-nextest is installed
if ! command -v cargo-nextest &>/dev/null; then
  echo "cargo-nextest not found; installing (may take a minute)..."
  cargo install cargo-nextest --locked || true
fi

# 4) Run workspace tests with nextest
run_step "workspace-tests" bash -lc 'cargo nextest run --workspace --all-features --retries 2' || true

# 5) Integration tests requiring a database
# Prefer DATABASE_URL from .env.local, then environment. Do NOT start Docker here.
DB_URL=""
if [ -f .env.local ]; then
  DB_URL_LINE=$(grep -m1 '^DATABASE_URL=' .env.local || true)
  if [ -n "$DB_URL_LINE" ]; then
    DB_URL=${DB_URL_LINE#DATABASE_URL=}
    # strip possible surrounding quotes
    DB_URL=${DB_URL%"}
    DB_URL=${DB_URL#"}
  fi
fi
if [ -z "${DB_URL:-}" ] && [ -n "${DATABASE_URL:-}" ]; then
  DB_URL="$DATABASE_URL"
fi

if [ -n "${DB_URL:-}" ]; then
  echo "Using DATABASE_URL from .env.local or environment for integration tests"
  export DATABASE_URL="$DB_URL"
  export RUST_TEST_THREADS=1
  run_step "integration-tests" bash -lc 'cargo nextest run --tests --workspace --all-features --retries 2' || true
else
  echo "No DATABASE_URL found in .env.local or environment; skipping DB-backed integration tests."
  echo "Set DATABASE_URL to your Neon dev branch connection string (or populate .env.local) to run them."
  status["integration-tests"]="skipped"
fi

# 6) Coverage (optional but run by default if cargo-llvm-cov is installed)
if [ "${SKIP_COVERAGE:-0}" != "1" ]; then
  if ! command -v cargo-llvm-cov &>/dev/null; then
    echo "cargo-llvm-cov not found; installing..."
    cargo install cargo-llvm-cov --locked || true
  fi
  run_step "coverage" bash -lc 'cargo llvm-cov --locked --all-features --lcov --output-path lcov.info --workspace --no-fail-fast' || true
  if [ -f lcov.info ]; then
    echo "Coverage file: $ROOT_DIR/lcov.info"
  fi
else
  echo "Skipping coverage (SKIP_COVERAGE=1)"
  status["coverage"]="skipped"
fi

# Summary report
echo
echo "================== autotests summary =================="
fail_count=0
for k in "fmt-check" "clippy" "workspace-tests" "integration-tests" "coverage"; do
  rc=${status[$k]:-"not-run"}
  if [ "$rc" = "0" ]; then
    printf "- %-18s : PASS\n" "$k"
  elif [ "$rc" = "skipped" ]; then
    printf "- %-18s : SKIPPED\n" "$k"
  else
    printf "- %-18s : FAIL (code=%s) -> see %s/%s.log\n" "$k" "$rc" "$LOGDIR" "${k// /_}"
    fail_count=$((fail_count+1))
  fi
done

echo
if [ $fail_count -eq 0 ]; then
  echo "All steps passed (or were skipped)."
  exit 0
fi

echo "Failures detected: $fail_count"
echo
echo "Suggested fixes:"
if [ ${status["fmt-check"]:-1} -ne 0 ]; then
  echo "- Formatting failed: run 'cargo fmt --all' to auto-format files."
fi
if [ ${status["clippy"]:-1} -ne 0 ]; then
  echo "- Clippy found issues: run 'cargo clippy --all-targets --all-features' and address warnings/errors."
fi
if [ ${status["workspace-tests"]:-1} -ne 0 ]; then
  echo "- Tests failed: run the failing tests locally with 'cargo nextest run' or inspect logs in $LOGDIR."
fi
if [ ${status["integration-tests"]:-1} -ne 0 ] && [ "${status["integration-tests"]}" != "skipped" ]; then
  echo "- Integration tests failed: ensure Postgres is running and DATABASE_URL is set. Review $LOGDIR/integration-tests.log."
fi
if [ ${status["coverage"]:-1} -ne 0 ] && [ "${status["coverage"]}" != "skipped" ]; then
  echo "- Coverage generation failed: ensure 'cargo-llvm-cov' is installed and tests pass."
fi

echo
echo "Detailed logs are available under: $LOGDIR"

exit 2
