#!/bin/bash

# AI Studio Proxy - Rust版本构建脚本

set -e

echo "🚀 开始构建Rust版本的AI Studio代理服务器..."

# 检查Rust是否安装
if ! command -v cargo &> /dev/null; then
    echo "❌ 错误: 未找到Rust/Cargo，请先安装Rust"
    echo "   安装命令: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# 检查Docker是否安装
if ! command -v docker &> /dev/null; then
    echo "❌ 错误: 未找到Docker，请先安装Docker"
    exit 1
fi

echo "✅ 环境检查通过"

# 进入Rust目录
cd rust

echo "📦 构建Rust项目..."
cargo build --release

echo "✅ Rust项目构建完成"

# 返回根目录
cd ..

echo "🐳 构建Docker镜像..."
docker build -f Dockerfile.rust -t aistudio-proxy-rust .

echo "✅ Docker镜像构建完成"

echo ""
echo "🎉 构建完成！"
echo ""
echo "使用方法："
echo "1. 本地运行:"
echo "   cd rust && cargo run"
echo ""
echo "2. Docker运行:"
echo "   docker run -p 5345:5345 -e AUTH_API_KEY=your_api_key_here aistudio-proxy-rust"
echo ""
echo "3. Docker Compose运行:"
echo "   docker-compose -f docker-compose.rust.yml up -d"
echo ""
echo "4. 测试连接:"
echo "   python3 rust/test_client.py"
