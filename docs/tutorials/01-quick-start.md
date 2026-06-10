# 快速开始

> **5 分钟上手 BeeBotOS**

本教程将帮助您在 5 分钟内完成 BeeBotOS 的安装和第一个 Agent 的创建。

---

## 目录

1. [环境准备](#环境准备)
2. [安装 BeeBotOS](#安装-beebotos)
3. [创建第一个 Agent](#创建第一个-agent)
4. [与 Agent 对话](#与-agent-对话)
5. [下一步](#下一步)

---

## 环境准备

### 系统要求

| 组件 | 最低要求 | 推荐配置 |
|------|---------|---------|
| 操作系统 | Linux / macOS / Windows WSL | Ubuntu 22.04 LTS |
| CPU | 2 核 | 4 核 |
| 内存 | 4 GB | 8 GB |
| 磁盘 | 10 GB 可用空间 | 50 GB SSD |

### 安装依赖

#### 1. 安装 Rust

```bash
# Linux/macOS
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 验证安装
rustc --version
cargo --version
```

#### 2. 安装 Docker (可选但推荐)

```bash
# Ubuntu
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker $USER

# macOS
brew install docker

# 验证
docker --version
```

---

## 安装 BeeBotOS

### 方式一：使用 Docker (推荐)

```bash
# 拉取最新镜像
docker pull beebotos/beebotos:latest

# 运行容器
docker run -d \
  --name beebotos \
  -p 8080:8080 \
  -v $(pwd)/data:/data \
  beebotos/beebotos:latest

# 检查状态
docker logs -f beebotos
```

### 方式二：从源码编译

```bash
# 克隆仓库
git clone https://github.com/linc77/beebotos.git
cd beebotos

# 编译 (可能需要 10-20 分钟)
cargo build --release

# 运行
./target/release/beebotos-server
```

### 方式三：使用安装脚本

```bash
# 下载安装脚本
curl -fsSL https://beebotos.io/install.sh | bash

# 验证安装
beebotos --version
```

---

## 创建第一个 Agent

### 1. 使用 CLI 创建 Agent

```bash
# 创建 Agent
beebotos-cli agent create \
  --name "MyFirstAgent" \
  --description "我的第一个 AI Agent" \
  --personality "friendly,helpful"

# 输出示例
# Agent created successfully!
# Agent ID: agent_abc123xyz
# Wallet Address: 0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb
```

### 2. 使用配置文件创建

创建 `my-agent.yaml`:

```yaml
name: "MyFirstAgent"
description: "我的第一个 AI Agent"

personality:
  type: "friendly"
  pad:
    pleasure: 0.5
    arousal: 0.3
    dominance: 0.4
  
  ocean:
    openness: 0.7
    conscientiousness: 0.8
    extraversion: 0.6
    agreeableness: 0.9
    neuroticism: 0.2

capabilities:
  - L3_NetworkOut
  - L7_ChainRead

resources:
  memory_mb: 256
  cpu_quota: 500
```

创建 Agent:

```bash
beebotos-cli agent create --config my-agent.yaml
```

### 3. 查看 Agent 列表

```bash
# 列出所有 Agent
beebotos-cli agent list

# 查看详情
beebotos-cli agent show agent_abc123xyz
```

---

## 与 Agent 对话

### 1. 使用 CLI 对话

```bash
# 发送消息
beebotos-cli agent chat agent_abc123xyz \
  --message "你好，请介绍一下自己"

# 交互式对话
beebotos-cli agent chat agent_abc123xyz --interactive
```

### 2. 使用 HTTP API

```bash
# 发送消息
curl -X POST http://localhost:8080/agents/agent_abc123xyz/message \
  -H "Content-Type: application/json" \
  -d '{
    "message": "你好，今天天气怎么样？"
  }'

# 响应示例
{
  "code": 200,
  "data": {
    "response": "你好！我目前无法获取实时天气信息，但我可以帮你查询。请告诉我你在哪个城市？",
    "emotion": {
      "pleasure": 0.6,
      "arousal": 0.4,
      "dominance": 0.3
    }
  }
}
```

### 3. 使用 WebSocket (实时流式响应)

```javascript
const ws = new WebSocket('ws://localhost:8080/ws');

ws.onopen = () => {
  ws.send(JSON.stringify({
    type: 'chat',
    agent_id: 'agent_abc123xyz',
    message: '你好！'
  }));
};

ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  console.log(data.response);
};
```

---

## 下一步

恭喜！您已经成功创建并运行了第一个 BeeBotOS Agent。接下来可以：

### 📚 学习更多

- [创建第一个 Agent](02-create-first-agent.md) - 深入了解 Agent 配置
- [开发技能](03-develop-skill.md) - 为 Agent 添加自定义功能
- [A2A 商业交易](04-a2a-commerce.md) - 让 Agent 自主赚钱

### 🛠️ 进阶操作

```bash
# 给 Agent 添加技能
beebotos-cli agent add-skill agent_abc123xyz \
  --skill weather-query \
  --config '{"api_key": "your-key"}'

# 导出 Agent 配置
beebotos-cli agent export agent_abc123xyz > my-agent-backup.yaml

# 部署到主网
beebotos-cli agent deploy agent_abc123xyz --network mainnet
```

### 🆘 需要帮助

- [故障排查指南](../guides/troubleshooting.md)
- [Discord 社区](https://discord.gg/beebotos)
- [GitHub Issues](https://github.com/linc77/beebotos/issues)

---

**预计时间**: 5 分钟  
**难度**: ⭐ 入门  
**前置知识**: 基础命令行操作
