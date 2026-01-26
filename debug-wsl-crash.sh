#!/bin/bash
# Comprehensive WSL crash debugging wrapper
# Monitors memory, processes, compilation state, and logs everything
#
# Usage:
#   ./debug-wsl-crash.sh

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR" || exit 1

LOG_FILE="/home/badinoff/projects/leadsnebula/.cursor/debug.log"

# Logging function
log_debug() {
    local hypothesis_id="$1"
    local location="$2"
    local message="$3"
    local data="$4"
    local timestamp=$(date +%s%3N 2>/dev/null || echo "$(date +%s)000")
    echo "{\"id\":\"log_${timestamp}_$$\",\"timestamp\":${timestamp},\"location\":\"${location}\",\"message\":\"${message}\",\"data\":${data},\"sessionId\":\"wsl-crash-debug\",\"runId\":\"run1\",\"hypothesisId\":\"${hypothesis_id}\"}" >> "$LOG_FILE" 2>/dev/null || true
}

# Clear previous logs
rm -f "$LOG_FILE"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🔍 WSL Crash Debug Monitor"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Monitoring:"
echo "  - Memory usage (every 50ms)"
echo "  - Process count (cargo/rustc)"
echo "  - Active compilation crates"
echo "  - System load"
echo "  - Logging to: $LOG_FILE"
echo ""

# Initial state logging
log_debug "A" "debug-wsl-crash.sh:init" "Debug session started" "{\"pid\":$$,\"wsl\":true}"
log_debug "B" "debug-wsl-crash.sh:init" "Debug session started" "{\"pid\":$$,\"wsl\":true}"
log_debug "C" "debug-wsl-crash.sh:init" "Debug session started" "{\"pid\":$$,\"wsl\":true}"
log_debug "D" "debug-wsl-crash.sh:init" "Debug session started" "{\"pid\":$$,\"wsl\":true}"
log_debug "E" "debug-wsl-crash.sh:init" "Debug session started" "{\"pid\":$$,\"wsl\":true}"

# Memory monitoring function (runs in background)
monitor_memory() {
    local last_available=0
    while true; do
        if [ -f /proc/meminfo ]; then
            local meminfo=$(cat /proc/meminfo)
            local mem_total=$(echo "$meminfo" | grep "^MemTotal:" | awk '{print $2}')
            local mem_available=$(echo "$meminfo" | grep "^MemAvailable:" | awk '{print $2}')
            local mem_free=$(echo "$meminfo" | grep "^MemFree:" | awk '{print $2}')
            local mem_cached=$(echo "$meminfo" | grep "^Cached:" | awk '{print $2}')
            local mem_buffers=$(echo "$meminfo" | grep "^Buffers:" | awk '{print $2}')
            local swap_total=$(echo "$meminfo" | grep "^SwapTotal:" | awk '{print $2}')
            local swap_free=$(echo "$meminfo" | grep "^SwapFree:" | awk '{print $2}')
            
            # Calculate used memory
            local mem_used=$((mem_total - mem_available))
            local mem_used_mb=$((mem_used / 1024))
            local mem_available_mb=$((mem_available / 1024))
            
            # Detect memory drops (potential crash indicator)
            if [ $mem_available_mb -lt $last_available ]; then
                local drop=$((last_available - mem_available_mb))
                if [ $drop -gt 100 ]; then
                    log_debug "A" "monitor_memory:spike" "Memory drop detected" "{\"available_mb\":${mem_available_mb},\"drop_mb\":${drop},\"used_mb\":${mem_used_mb}}"
                fi
            fi
            last_available=$mem_available_mb
            
            # Log memory state
            log_debug "A" "monitor_memory:state" "Memory state" "{\"total_mb\":$((mem_total/1024)),\"available_mb\":${mem_available_mb},\"used_mb\":${mem_used_mb},\"free_mb\":$((mem_free/1024)),\"cached_mb\":$((mem_cached/1024)),\"buffers_mb\":$((mem_buffers/1024)),\"swap_total_mb\":$((swap_total/1024)),\"swap_free_mb\":$((swap_free/1024))}"
        fi
        sleep 0.05  # 50ms interval
    done
}

# Process monitoring function (runs in background)
monitor_processes() {
    while true; do
        cargo_count=$(pgrep -c cargo 2>/dev/null || echo "0")
        rustc_count=$(pgrep -c rustc 2>/dev/null || echo "0")
        clang_count=$(pgrep -c clang 2>/dev/null || echo "0")
        cursor_count=$(pgrep -c -f "cursor-server\|cursor-reh" 2>/dev/null || echo "0")
        total_compilation=$((cargo_count + rustc_count + clang_count))
        
        log_debug "B" "monitor_processes:count" "Process count" "{\"cargo\":${cargo_count},\"rustc\":${rustc_count},\"clang\":${clang_count},\"cursor\":${cursor_count},\"total_compilation\":${total_compilation}}"
        log_debug "D" "monitor_processes:cursor" "Cursor server state" "{\"cursor_procs\":${cursor_count}}"
        
        # Track process memory usage
        if [ $rustc_count -gt 0 ]; then
            rustc_mem=$(pgrep rustc | xargs -r ps -o rss= -p 2>/dev/null | awk '{sum+=$1} END {print sum+0}')
            log_debug "B" "monitor_processes:rustc_mem" "Rustc memory usage" "{\"total_kb\":${rustc_mem},\"count\":${rustc_count}}"
        fi
        
        if [ $cursor_count -gt 0 ]; then
            cursor_mem=$(pgrep -f "cursor-server\|cursor-reh" | xargs -r ps -o rss= -p 2>/dev/null | awk '{sum+=$1} END {print sum+0}')
            log_debug "D" "monitor_processes:cursor_mem" "Cursor server memory" "{\"total_kb\":${cursor_mem},\"count\":${cursor_count}}"
        fi
        
        sleep 0.1  # 100ms interval
    done
}

# Compilation tracking (monitors cargo output)
monitor_compilation() {
    local last_crate=""
    while IFS= read -r line; do
        # Extract crate name from "Compiling crate-name" lines
        if echo "$line" | grep -q "^   Compiling "; then
            local crate=$(echo "$line" | sed -n 's/^   Compiling \([^ ]*\).*/\1/p')
            if [ -n "$crate" ] && [ "$crate" != "$last_crate" ]; then
                log_debug "C" "monitor_compilation:crate" "Compiling crate" "{\"crate\":\"${crate}\",\"line\":\"${line}\"}"
                last_crate="$crate"
            fi
        fi
        
        # Track linking phase
        if echo "$line" | grep -q "Linking\|Finished\|Running"; then
            mem_during_link=$(grep "^MemAvailable:" /proc/meminfo 2>/dev/null | awk '{print $2}' || echo "0")
            log_debug "E" "monitor_compilation:phase" "Compilation phase" "{\"phase\":\"$(echo "$line" | cut -c1-50)\",\"mem_available_kb\":${mem_during_link}}"
        fi
        
        # Track errors
        if echo "$line" | grep -qi "error\|panic\|fatal"; then
            log_debug "A" "monitor_compilation:error" "Compilation error" "{\"line\":\"$(echo "$line" | cut -c1-200)\"}"
        fi
    done
}

# System load monitoring
monitor_load() {
    while true; do
        if [ -f /proc/loadavg ]; then
            local load=$(cat /proc/loadavg)
            local load_1min=$(echo "$load" | awk '{print $1}')
            local load_5min=$(echo "$load" | awk '{print $2}')
            local load_15min=$(echo "$load" | awk '{print $3}')
            local running_procs=$(echo "$load" | awk '{print $4}' | cut -d'/' -f1)
            local total_procs=$(echo "$load" | awk '{print $4}' | cut -d'/' -f2)
            
            log_debug "B" "monitor_load:state" "System load" "{\"load_1min\":${load_1min},\"load_5min\":${load_5min},\"load_15min\":${load_15min},\"running_procs\":${running_procs},\"total_procs\":${total_procs}}"
        fi
        sleep 0.2  # 200ms interval
    done
}

# Start background monitors
monitor_memory &
MEM_PID=$!
monitor_processes &
PROC_PID=$!
monitor_load &
LOAD_PID=$!

# Cleanup function
cleanup() {
    echo ""
    echo "🧹 Stopping monitors..."
    kill $MEM_PID $PROC_PID $LOAD_PID 2>/dev/null || true
    wait $MEM_PID $PROC_PID $LOAD_PID 2>/dev/null || true
    
    log_debug "A" "debug-wsl-crash.sh:cleanup" "Debug session ended" "{\"exit_code\":${1:-0}}"
    log_debug "B" "debug-wsl-crash.sh:cleanup" "Debug session ended" "{\"exit_code\":${1:-0}}"
    log_debug "C" "debug-wsl-crash.sh:cleanup" "Debug session ended" "{\"exit_code\":${1:-0}}"
    log_debug "D" "debug-wsl-crash.sh:cleanup" "Debug session ended" "{\"exit_code\":${1:-0}}"
    log_debug "E" "debug-wsl-crash.sh:cleanup" "Debug session ended" "{\"exit_code\":${1:-0}}"
    
    echo "✅ Logs saved to: $LOG_FILE"
    echo ""
    echo "📊 Analysis commands:"
    echo "   # Memory drops:"
    echo "   grep 'Memory drop' $LOG_FILE | jq -r '.data'"
    echo "   # Peak memory usage:"
    echo "   grep 'Memory state' $LOG_FILE | jq -r '.data.used_mb' | sort -n | tail -5"
    echo "   # Process counts:"
    echo "   grep 'Process count' $LOG_FILE | jq -r '.data' | tail -20"
    echo "   # Compiling crates:"
    echo "   grep 'Compiling crate' $LOG_FILE | jq -r '.data.crate' | tail -10"
}

trap 'cleanup $?' EXIT INT TERM

# Log before running autotestsall.sh
log_debug "A" "debug-wsl-crash.sh:start" "Starting autotestsall.sh" "{\"script\":\"autotestsall.sh\"}"
log_debug "B" "debug-wsl-crash.sh:start" "Starting autotestsall.sh" "{\"script\":\"autotestsall.sh\"}"
log_debug "C" "debug-wsl-crash.sh:start" "Starting autotestsall.sh" "{\"script\":\"autotestsall.sh\"}"
log_debug "D" "debug-wsl-crash.sh:start" "Starting autotestsall.sh" "{\"script\":\"autotestsall.sh\"}"
log_debug "E" "debug-wsl-crash.sh:start" "Starting autotestsall.sh" "{\"script\":\"autotestsall.sh\"}"

# Run autotestsall.sh with compilation monitoring
echo "🚀 Running autotestsall.sh with full monitoring..."
echo ""

./autotestsall.sh 2>&1 | tee >(monitor_compilation)

EXIT_CODE=${PIPESTATUS[0]}

log_debug "A" "debug-wsl-crash.sh:end" "autotestsall.sh completed" "{\"exit_code\":${EXIT_CODE}}"
log_debug "B" "debug-wsl-crash.sh:end" "autotestsall.sh completed" "{\"exit_code\":${EXIT_CODE}}"
log_debug "C" "debug-wsl-crash.sh:end" "autotestsall.sh completed" "{\"exit_code\":${EXIT_CODE}}"
log_debug "D" "debug-wsl-crash.sh:end" "autotestsall.sh completed" "{\"exit_code\":${EXIT_CODE}}"
log_debug "E" "debug-wsl-crash.sh:end" "autotestsall.sh completed" "{\"exit_code\":${EXIT_CODE}}"

exit $EXIT_CODE
