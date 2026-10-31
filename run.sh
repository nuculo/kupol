#!/usr/bin/env bash
# ============================================================================
# run.sh — run the KUPOL engine (binary: duo-agents) from a local build
# ============================================================================
# Usage:
#   ./run.sh              # Web UI + API (http://localhost:3000)
#   ./run.sh scan         # scan the current project
#   ./run.sh scan <path>  # scan a specific path
#   ./run.sh demo         # run the multi-agent demo scenarios
#   ./run.sh info         # engine information
#   ./run.sh report       # export a report
#   ./run.sh help         # this help
# ============================================================================

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Detect the engine binary.
if [ -f "target/release/duo-agents" ]; then
    BIN="target/release/duo-agents"
elif [ -f "target/debug/duo-agents" ]; then
    BIN="target/debug/duo-agents"
else
    echo -e "${RED}Engine binary not found. Build it first: ./build.sh${NC}"
    exit 1
fi

CMD="${1:-serve}"
shift 2>/dev/null || true

case "$CMD" in
    serve|ui|web|start)
        echo -e "${CYAN}KUPOL — web server${NC}"
        $BIN serve --port "${PORT:-3000}" "$@"
        ;;

    scan)
        TARGET="${1:-src/}"
        shift 2>/dev/null || true
        $BIN scan --path "$TARGET" "$@"
        ;;

    scan-json)
        TARGET="${1:-src/}"
        shift 2>/dev/null || true
        $BIN scan --path "$TARGET" --format json "$@"
        ;;

    scan-md)
        TARGET="${1:-src/}"
        shift 2>/dev/null || true
        $BIN scan --path "$TARGET" --format markdown "$@"
        ;;

    demo)
        $BIN demo "$@"
        ;;

    info)
        $BIN info
        ;;

    report)
        $BIN report "$@"
        ;;

    help|-h|--help)
        echo -e "${CYAN}KUPOL — help${NC}"
        echo ""
        echo -e "  ${GREEN}serve${NC}  (default)    Web UI + API (http://localhost:3000)"
        echo -e "  ${GREEN}scan${NC}   [path]       scan code for security findings"
        echo -e "  ${GREEN}scan-json${NC} [path]    scan with JSON output"
        echo -e "  ${GREEN}scan-md${NC}  [path]     scan with Markdown output"
        echo -e "  ${GREEN}demo${NC}               run the multi-agent demo scenarios"
        echo -e "  ${GREEN}info${NC}               engine information"
        echo -e "  ${GREEN}report${NC}             export the last report"
        echo -e "  ${GREEN}help${NC}               this help"
        echo ""
        echo -e "  ${CYAN}Examples:${NC}"
        echo -e "    ./run.sh                     ${YELLOW}# Web UI + API${NC}"
        echo -e "    ./run.sh scan                ${YELLOW}# scan the current project${NC}"
        echo -e "    ./run.sh scan /path/to/code  ${YELLOW}# scan a specific path${NC}"
        echo -e "    ./run.sh scan-json src/ > report.json"
        echo -e "    PORT=8080 ./run.sh           ${YELLOW}# custom port${NC}"
        echo ""
        ;;

    *)
        echo -e "${RED}Unknown command: $CMD${NC}"
        echo "Use: ./run.sh help"
        exit 1
        ;;
esac
