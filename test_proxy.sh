#!/bin/bash

# 测试 AI Studio Proxy 的脚本
# 需要先设置 AUTH_API_KEY 环境变量

# 颜色定义
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 配置
PROXY_URL="http://127.0.0.1:5345"
AUTH_API_KEY="${AUTH_API_KEY:-your_set_api_key_here}"

echo -e "${YELLOW}=== AI Studio Proxy 测试工具 ===${NC}\n"

# 测试 1: 非流式请求 - 生成内容
echo -e "${YELLOW}测试 1: 非流式请求 (generateContent)${NC}"
curl -X POST "${PROXY_URL}/v1beta/models/gemini-2.0-flash-exp:generateContent" \
  -H "Content-Type: application/json" \
  -H "x-goog-api-key: ${AUTH_API_KEY}" \
  -d '{
    "contents": [{
      "parts": [{
        "text": "用一句话解释什么是 WebSocket"
      }]
    }]
  }' \
  -w "\n\n状态码: %{http_code}\n" \
  -s | jq '.' 2>/dev/null || cat

echo -e "\n${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}\n"

# 测试 2: 流式请求 - 使用 streamGenerateContent
echo -e "${YELLOW}测试 2: 流式请求 (streamGenerateContent)${NC}"
echo "发送请求并接收 SSE 流..."
curl -X POST "${PROXY_URL}/v1beta/models/gemini-2.0-flash-exp:streamGenerateContent?alt=sse" \
  -H "Content-Type: application/json" \
  -H "x-goog-api-key: ${AUTH_API_KEY}" \
  -d '{
    "contents": [{
      "parts": [{
        "text": "数到 5"
      }]
    }]
  }' \
  -N \
  -s | head -20

echo -e "\n${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}\n"

# 测试 3: 列出模型
echo -e "${YELLOW}测试 3: 列出可用模型${NC}"
curl -X GET "${PROXY_URL}/v1beta/models?key=${AUTH_API_KEY}" \
  -w "\n\n状态码: %{http_code}\n" \
  -s | jq '.models[] | {name: .name, displayName: .displayName}' 2>/dev/null | head -20

echo -e "\n${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}\n"

# 测试 4: 检查认证失败
echo -e "${YELLOW}测试 4: 认证失败测试 (应该返回 401)${NC}"
curl -X POST "${PROXY_URL}/v1beta/models/gemini-2.0-flash-exp:generateContent" \
  -H "Content-Type: application/json" \
  -H "x-goog-api-key: wrong_key" \
  -d '{"contents": [{"parts": [{"text": "test"}]}]}' \
  -w "\n状态码: %{http_code}\n" \
  -s

echo -e "\n${GREEN}=== 测试完成 ===${NC}"