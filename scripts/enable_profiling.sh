#!/bin/bash
# Enable tracing-flame profiling for performance analysis
# Usage: ./scripts/enable_profiling.sh [output_file]

set -e

OUTPUT_FILE="${1:-flamegraph.svg}"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Enabling tracing-flame profiling"
echo "Output file: $OUTPUT_FILE"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "To enable profiling, set these environment variables:"
echo ""
echo "  export RUST_LOG=info"
echo "  export TRACING_FLAME_OUTPUT=$OUTPUT_FILE"
echo ""
echo "Then run your application. The flamegraph will be generated at:"
echo "  $OUTPUT_FILE"
echo ""
echo "To view the flamegraph, install flamegraph:"
echo "  cargo install flamegraph"
echo "  flamegraph --flamechart $OUTPUT_FILE"
echo ""
