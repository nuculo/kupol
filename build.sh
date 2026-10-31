#!/usr/bin/env bash
# ============================================================================
# build.sh — build KUPOL (engine crate: duo-agents)
# ============================================================================
# Usage:
#   ./build.sh              # full build (Rust engine + dashboard)
#   ./build.sh --rust       # Rust engine only
#   ./build.sh --dashboard  # React dashboard only
#   ./build.sh --release    # release optimization
#   ./build.sh --clean      # clean build
# ============================================================================

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

RUST_ONLY=false
DASH_ONLY=false
RELEASE=false
CLEAN=false

for arg in "$@"; do
    case $arg in
        --rust) RUST_ONLY=true ;;
        --dashboard|--ui) DASH_ONLY=true ;;
        --release) RELEASE=true ;;
        --clean) CLEAN=true ;;
        --help|-h)
            echo -e "${CYAN}build.sh — build KUPOL${NC}"
            echo ""
            echo "  --rust         Rust engine only"
            echo "  --dashboard    React dashboard only"
            echo "  --release      release optimization"
            echo "  --clean        clean build"
            exit 0 ;;
        *) echo -e "${RED}Unknown argument: $arg${NC}"; exit 1 ;;
    esac
done

echo -e "${CYAN}Building KUPOL${NC}"

START_TIME=$(date +%s)

if [ "$CLEAN" = true ]; then
    echo -e "${YELLOW}Cleaning...${NC}"
    cargo clean 2>/dev/null || true
    rm -rf dashboard/dist dashboard/node_modules 2>/dev/null || true
    echo -e "${GREEN}Clean complete${NC}"
fi

# Rust engine
if [ "$DASH_ONLY" = false ]; then
    echo -e "${CYAN}Building Rust engine...${NC}"
    if [ "$RELEASE" = true ]; then
        cargo build --release 2>&1 | tail -5
        BIN="target/release/duo-agents"
    else
        cargo build 2>&1 | tail -5
        BIN="target/debug/duo-agents"
    fi
    echo -e "${GREEN}Engine built: $BIN${NC}"
    ./"$BIN" info 2>/dev/null | head -2 || true
fi

# React dashboard
if [ "$RUST_ONLY" = false ] && [ -d "dashboard" ]; then
    echo ""
    echo -e "${CYAN}Building React dashboard...${NC}"
    cd dashboard
    if [ ! -d "node_modules" ]; then
        npm install --prefer-offline 2>&1 | tail -3
    fi
    npm run build 2>&1 | tail -5
    cd ..
    echo -e "${GREEN}Dashboard built: dashboard/dist/${NC}"
fi

END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))

echo ""
echo -e "${GREEN}Build finished in ${ELAPSED}s${NC}"
echo ""
echo -e "  ${YELLOW}Serve:${NC}  ./run.sh"
echo -e "  ${YELLOW}Scan:${NC}   ./run.sh scan"
echo -e "  ${YELLOW}Demo:${NC}   ./run.sh demo"
echo ""
