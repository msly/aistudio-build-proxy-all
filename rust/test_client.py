#!/usr/bin/env python3
"""
测试Rust代理服务器的客户端脚本
"""

import asyncio
import websockets
import json
import requests
import time

async def test_websocket():
    """测试WebSocket连接"""
    uri = "ws://localhost:5345/v1/ws?auth_token=valid-token-user-1"
    
    try:
        async with websockets.connect(uri) as websocket:
            print("✅ WebSocket连接成功")
            
            # 发送ping消息
            ping_msg = {
                "id": "test-ping-1",
                "type": "ping",
                "payload": None
            }
            await websocket.send(json.dumps(ping_msg))
            print("📤 发送ping消息")
            
            # 接收pong响应
            response = await websocket.recv()
            pong_msg = json.loads(response)
            print(f"📥 收到响应: {pong_msg}")
            
            if pong_msg.get("type") == "pong":
                print("✅ Ping-Pong测试成功")
            else:
                print("❌ Ping-Pong测试失败")
                
    except Exception as e:
        print(f"❌ WebSocket连接失败: {e}")

def test_http_proxy():
    """测试HTTP代理"""
    url = "http://localhost:5345/v1/models/gemini-pro:generate"
    headers = {
        "x-goog-api-key": "your_set_api_key_here",
        "Content-Type": "application/json"
    }
    data = {
        "contents": [
            {
                "parts": [
                    {
                        "text": "Hello, this is a test message"
                    }
                ]
            }
        ]
    }
    
    try:
        print("📤 发送HTTP请求...")
        response = requests.post(url, headers=headers, json=data, timeout=30)
        print(f"📥 HTTP响应状态: {response.status_code}")
        print(f"📥 HTTP响应内容: {response.text[:200]}...")
        
        if response.status_code == 200:
            print("✅ HTTP代理测试成功")
        else:
            print(f"❌ HTTP代理测试失败: {response.status_code}")
            
    except Exception as e:
        print(f"❌ HTTP请求失败: {e}")

async def main():
    """主测试函数"""
    print("🚀 开始测试Rust代理服务器...")
    print("=" * 50)
    
    # 测试WebSocket
    print("1. 测试WebSocket连接...")
    await test_websocket()
    print()
    
    # 等待一下
    await asyncio.sleep(1)
    
    # 测试HTTP代理
    print("2. 测试HTTP代理...")
    test_http_proxy()
    print()
    
    print("=" * 50)
    print("🎉 测试完成!")

if __name__ == "__main__":
    asyncio.run(main())
