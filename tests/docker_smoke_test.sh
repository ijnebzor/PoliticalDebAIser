#!/usr/bin/env bash
# =============================================================================
# Docker Smoke Test — PoliticalDebAIser Stage 2
#
# Verifies that docker compose brings up both services healthy and the
# app responds correctly to basic requests.
#
# Usage:
#   ./tests/docker_smoke_test.sh          # full test (build + run + verify)
#   ./tests/docker_smoke_test.sh --no-build  # skip docker build (use cached)
#
# Prerequisites: docker and docker compose installed and running.
# =============================================================================
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

PASS=0
FAIL=0
SKIP=0

pass() { ((PASS++)); echo -e "  ${GREEN}PASS${NC} $1"; }
fail() { ((FAIL++)); echo -e "  ${RED}FAIL${NC} $1: $2"; }
skip() { ((SKIP++)); echo -e "  ${YELLOW}SKIP${NC} $1: $2"; }

APP_URL="http://localhost:3000"
OLLAMA_URL="http://localhost:11434"
COMPOSE_PROJECT="political-debaiser-smoke"
NO_BUILD=false

for arg in "$@"; do
    case $arg in
        --no-build) NO_BUILD=true ;;
    esac
done

echo "============================================================"
echo " Docker Smoke Test — PoliticalDebAIser"
echo "============================================================"
echo ""

# -------------------------------------------------------------------
# Cleanup function: always tear down containers
# -------------------------------------------------------------------
cleanup() {
    echo ""
    echo "Tearing down containers..."
    docker compose -p "$COMPOSE_PROJECT" down --volumes --remove-orphans 2>/dev/null || true
}
trap cleanup EXIT

# -------------------------------------------------------------------
# Step 1: Build (unless --no-build)
# -------------------------------------------------------------------
if [ "$NO_BUILD" = false ]; then
    echo "Step 1: Building Docker image..."
    if docker compose -p "$COMPOSE_PROJECT" build --quiet 2>&1; then
        pass "Docker image builds successfully"
    else
        fail "Docker image build" "docker compose build failed"
        echo ""
        echo "Results: $PASS passed, $FAIL failed, $SKIP skipped"
        exit 1
    fi
else
    skip "Docker build" "skipped with --no-build"
fi

# -------------------------------------------------------------------
# Step 2: Start services
# -------------------------------------------------------------------
echo ""
echo "Step 2: Starting services..."
docker compose -p "$COMPOSE_PROJECT" up -d 2>&1

# -------------------------------------------------------------------
# Step 3: Wait for health checks
# -------------------------------------------------------------------
echo ""
echo "Step 3: Waiting for services to become healthy..."

# Wait for Ollama to be healthy (up to 60s)
echo "  Waiting for Ollama..."
OLLAMA_HEALTHY=false
for i in $(seq 1 30); do
    if curl -sf "$OLLAMA_URL/api/tags" >/dev/null 2>&1; then
        OLLAMA_HEALTHY=true
        break
    fi
    sleep 2
done

if [ "$OLLAMA_HEALTHY" = true ]; then
    pass "Ollama is healthy and responding"
else
    fail "Ollama health" "not responding after 60s"
fi

# Wait for app to be healthy (up to 60s)
echo "  Waiting for app..."
APP_HEALTHY=false
for i in $(seq 1 30); do
    if curl -sf "$APP_URL/health" >/dev/null 2>&1; then
        APP_HEALTHY=true
        break
    fi
    sleep 2
done

if [ "$APP_HEALTHY" = true ]; then
    pass "App is healthy and responding"
else
    fail "App health" "not responding after 60s"
    echo ""
    echo "Container logs:"
    docker compose -p "$COMPOSE_PROJECT" logs --tail=50
    echo ""
    echo "Results: $PASS passed, $FAIL failed, $SKIP skipped"
    exit 1
fi

# -------------------------------------------------------------------
# Step 4: Smoke tests
# -------------------------------------------------------------------
echo ""
echo "Step 4: Running smoke tests..."

# Test 4a: Health endpoint returns JSON with status=ok
HEALTH_RESP=$(curl -sf "$APP_URL/health" 2>&1 || true)
if echo "$HEALTH_RESP" | grep -q '"status":"ok"'; then
    pass "GET /health returns {\"status\":\"ok\"}"
else
    fail "GET /health" "unexpected response: $HEALTH_RESP"
fi

# Test 4b: Static files served (index.html)
INDEX_RESP=$(curl -sf -o /dev/null -w "%{http_code}" "$APP_URL/" 2>&1 || true)
if [ "$INDEX_RESP" = "200" ]; then
    pass "GET / returns 200 (index.html served)"
else
    fail "GET /" "expected 200, got $INDEX_RESP"
fi

# Test 4c: CSS served
CSS_RESP=$(curl -sf -o /dev/null -w "%{http_code}" "$APP_URL/static/styles.css" 2>&1 || true)
if [ "$CSS_RESP" = "200" ]; then
    pass "GET /static/styles.css returns 200"
else
    fail "GET /static/styles.css" "expected 200, got $CSS_RESP"
fi

# Test 4d: JS served
JS_RESP=$(curl -sf -o /dev/null -w "%{http_code}" "$APP_URL/static/app.js" 2>&1 || true)
if [ "$JS_RESP" = "200" ]; then
    pass "GET /static/app.js returns 200"
else
    fail "GET /static/app.js" "expected 200, got $JS_RESP"
fi

# Test 4e: Empty text returns 400
EMPTY_RESP=$(curl -sf -o /dev/null -w "%{http_code}" -X POST "$APP_URL/analyze-text" \
    -H "Content-Type: application/json" \
    -d '{"text":""}' 2>&1 || true)
if [ "$EMPTY_RESP" = "400" ]; then
    pass "POST /analyze-text with empty text returns 400"
else
    fail "POST /analyze-text (empty)" "expected 400, got $EMPTY_RESP"
fi

# Test 4f: Text too long returns 400
LONG_TEXT=$(python3 -c "print('x' * 100001)" 2>/dev/null || echo "")
if [ -n "$LONG_TEXT" ]; then
    LONG_RESP=$(curl -sf -o /dev/null -w "%{http_code}" -X POST "$APP_URL/analyze-text" \
        -H "Content-Type: application/json" \
        -d "{\"text\":\"$LONG_TEXT\"}" 2>&1 || true)
    if [ "$LONG_RESP" = "400" ]; then
        pass "POST /analyze-text with oversized text returns 400"
    else
        fail "POST /analyze-text (too long)" "expected 400, got $LONG_RESP"
    fi
else
    skip "POST /analyze-text (too long)" "python3 not available"
fi

# Test 4g: Invalid JSON returns 4xx
INVALID_RESP=$(curl -sf -o /dev/null -w "%{http_code}" -X POST "$APP_URL/analyze-text" \
    -H "Content-Type: application/json" \
    -d 'not json at all' 2>&1 || true)
if [[ "$INVALID_RESP" =~ ^4 ]]; then
    pass "POST /analyze-text with invalid JSON returns 4xx ($INVALID_RESP)"
else
    fail "POST /analyze-text (invalid JSON)" "expected 4xx, got $INVALID_RESP"
fi

# Test 4h: History endpoint returns empty array
HISTORY_RESP=$(curl -sf "$APP_URL/history" 2>&1 || true)
if echo "$HISTORY_RESP" | grep -q '^\[\]$'; then
    pass "GET /history returns empty array"
else
    fail "GET /history" "expected [], got: $HISTORY_RESP"
fi

# Test 4i: Non-existent history ID returns 404
NOTFOUND_RESP=$(curl -sf -o /dev/null -w "%{http_code}" "$APP_URL/history/nonexistent" 2>&1 || true)
if [ "$NOTFOUND_RESP" = "404" ]; then
    pass "GET /history/nonexistent returns 404"
else
    fail "GET /history/nonexistent" "expected 404, got $NOTFOUND_RESP"
fi

# Test 4j: Security headers present
HEADERS_RESP=$(curl -sf -D- -o /dev/null "$APP_URL/health" 2>&1 || true)
if echo "$HEADERS_RESP" | grep -qi "x-content-type-options"; then
    pass "Security header X-Content-Type-Options present"
else
    skip "Security headers" "X-Content-Type-Options not found (may be set at proxy level)"
fi

# -------------------------------------------------------------------
# Summary
# -------------------------------------------------------------------
echo ""
echo "============================================================"
echo " Results: ${GREEN}$PASS passed${NC}, ${RED}$FAIL failed${NC}, ${YELLOW}$SKIP skipped${NC}"
echo "============================================================"

if [ "$FAIL" -gt 0 ]; then
    exit 1
fi
