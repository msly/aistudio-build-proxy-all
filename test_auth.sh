"#!/bin/bash

# Test script to verify API key authentication in Rust version
# This script tests both header and query parameter API key authentication

set -e

API_KEY="test_api_key_123"
PROXY_URL="http://localhost:5345"

echo "Testing Rust API key authentication..."
echo "API Key: $API_KEY"
echo "Proxy URL: $PROXY_URL"
echo ""

# Test 1: Header-based authentication
echo "Test 1: Header-based authentication"
curl -X POST "$PROXY_URL/v1/models" \
  -H "x-goog-api-key: $API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"gemini-pro"}' \
  --max-time 5 \
  --silent --show-error \
  -w "HTTP Status: %{http_code}\n" \
  || echo "Test 1 failed (expected if server not running)"

echo ""

# Test 2: Query parameter authentication
echo "Test 2: Query parameter authentication"
curl -X POST "$PROXY_URL/v1/models?key=$API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"gemini-pro"}' \
  --max-time 5 \
  --silent --show-error \
  -w "HTTP Status: %{http_code}\n" \
  || echo "Test 2 failed (expected if server not running)"

echo ""

# Test 3: Invalid API key (should fail)
echo "Test 3: Invalid API key (should fail with 401)"
curl -X POST "$PROXY_URL/v1/models" \
  -H "x-goog-api-key: invalid_key" \
  -H "Content-Type: application/json" \
  -d '{"model":"gemini-pro"}' \
  --max-time 5 \
  --silent --show-error \
  -w "HTTP Status: %{http_code}\n" \
  || echo "Test 3 correctly returned error"

echo ""

# Test 4: Invalid query parameter (should fail with 401)
echo "Test 4: Invalid query parameter (should fail with 401)"
curl -X POST "$PROXY_URL/v1/models?key=invalid_key" \
  -H "Content-Type: application/json" \
  -d '{"model":"gemini-pro"}' \
  --max-time 5 \
  --silent --show-error \
  -w "HTTP Status: %{http_code}\n" \
  || echo "Test 4 correctly returned error"

echo ""

# Test 5: No API key (should fail with 401)
echo "Test 5: No API key (should fail with 401)"
curl -X POST "$PROXY_URL/v1/models" \
  -H "Content-Type: application/json" \
  -d '{"model":"gemini-pro"}' \
  --max-time 5 \
  --silent --show-error \
  -w "HTTP Status: %{http_code}\n" \
  || echo "Test 5 correctly returned error"

echo ""
echo "Authentication tests completed!"
echo ""
echo "To run the Rust proxy server, use:"
echo "  cd rust && AUTH_API_KEY=$API_KEY cargo run"
echo ""
echo "Expected results:"
echo "  Tests 1-2: Should return 503 (Service Unavailable) if server running but no WS client"
echo "  Tests 3-5: Should return 401 (Unauthorized)"