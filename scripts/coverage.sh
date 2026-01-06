#!/bin/bash
# Generate code coverage report locally
# Usage: ./scripts/coverage.sh

set -e

echo "🔍 Generating code coverage report..."
echo ""

# Check if cargo-llvm-cov is installed
if ! command -v cargo-llvm-cov &> /dev/null; then
    echo "Installing cargo-llvm-cov..."
    cargo install cargo-llvm-cov --locked
fi

# Generate coverage
echo "Running tests with coverage..."
cargo llvm-cov --locked --all-features --lcov --output-path lcov.info

# Generate HTML report
echo ""
echo "Generating HTML report..."
cargo llvm-cov --locked --all-features --html --output-dir coverage-html

echo ""
echo "✅ Coverage report generated!"
echo "   - LCOV format: lcov.info"
echo "   - HTML report: coverage-html/index.html"
echo ""
echo "Open coverage-html/index.html in your browser to view the report."
