# BeeBotOS API 文档

> **完整的 API 参考文档**

欢迎使用 BeeBotOS API 文档。本文档涵盖了与 BeeBotOS 交互的所有方式。

---

## 概述

BeeBotOS 提供多种 API 接口，满足不同场景的需求：

| API 类型 | 适用场景 | 实时性 | 协议 |
|---------|---------|--------|------|
| **REST API** | 常规操作 | 请求-响应 | HTTP/1.1, HTTP/2 |
| **gRPC API** | 高性能内部通信 | 请求-响应, 流式 | HTTP/2 |
| **WebSocket** | 实时事件 | 双向流式 | WebSocket |
| **A2A Protocol** | Agent 间通信 | 异步 | libp2p/WebSocket |
| **Kernel Syscall** | 系统级操作 | 同步 | 内部接口 |

---

## 快速导航

### 核心 API

- [REST API](rest-api.md) - HTTP API 完整参考
- [gRPC API](grpc.md) - 高性能 RPC 接口
- [WebSocket](rest-api.md#websocket) - 实时事件流

### 协议规范

- [A2A Protocol](a2a-protocol-spec.md) - Agent 间通信协议
- [DAO Protocol](dao-protocol-spec.md) - 治理协议
- [MCP Integration](mcp-integration.md) - 模型上下文协议

### 系统接口

- [Kernel Syscalls](kernel-syscall.md) - 系统调用接口
- [Authentication](authentication.md) - 认证与授权

---

## 认证方式

所有 API 请求都需要认证。支持的认证方式：

### API Key (快速开始)

```bash
curl -H "X-API-Key: your-api-key" \
  https://api.beebotos.io/v1/agents
```

### JWT Token (生产环境)

```bash
# 获取 Token
curl -X POST https://api.beebotos.io/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{
    "address": "0x...",
    "signature": "0x..."
  }'

# 使用 Token
curl -H "Authorization: Bearer eyJhbG..." \
  https://api.beebotos.io/v1/agents
```

详见 [Authentication](authentication.md)

---

## 基础 URL

| 环境 | URL |
|------|-----|
| 生产环境 | `https://api.beebotos.io/v1` |
| 测试环境 | `https://api-test.beebotos.io/v1` |
| 本地开发 | `http://localhost:8080/v1` |

---

## 标准响应格式

### 成功响应

```json
{
  "code": 200,
  "message": "success",
  "data": {
    // 响应数据
  },
  "request_id": "req_abc123",
  "timestamp": 1700000000000
}
```

### 错误响应

```json
{
  "code": 4001,
  "message": "Invalid parameter",
  "details": {
    "field": "name",
    "error": "required"
  },
  "request_id": "req_abc123",
  "timestamp": 1700000000000
}
```

### 错误代码

| 代码 | 含义 | HTTP 状态 |
|------|------|----------|
| 200 | 成功 | 200 |
| 4000 | 请求参数错误 | 400 |
| 4001 | 缺少必填参数 | 400 |
| 4002 | 参数格式错误 | 400 |
| 4010 | 未授权 | 401 |
| 4011 | Token 过期 | 401 |
| 4030 | 权限不足 | 403 |
| 4040 | 资源不存在 | 404 |
| 4090 | 资源冲突 | 409 |
| 4290 | 请求过于频繁 | 429 |
| 5000 | 服务器内部错误 | 500 |

---

## SDK

我们提供以下官方 SDK：

| 语言 | 包名 | 安装 |
|------|------|------|
| JavaScript/TypeScript | `@beebotos/sdk` | `npm install @beebotos/sdk` |
| Python | `beebotos` | `pip install beebotos` |
| Rust | `beebotos-sdk` | `cargo add beebotos-sdk` |
| Go | `github.com/beebotos/go-sdk` | `go get github.com/beebotos/go-sdk` |

---

## 示例代码

### JavaScript

```javascript
import { BeeBotOS } from '@beebotos/sdk';

const client = new BeeBotOS({
  apiKey: 'your-api-key'
});

// 创建 Agent
const agent = await client.agents.create({
  name: 'MyAgent',
  capabilities: ['L3_NetworkOut']
});

// 与 Agent 对话
const response = await client.agents.chat(agent.id, {
  message: 'Hello!'
});

console.log(response.data.response);
```

### Python

```python
from beebotos import BeeBotOS

client = BeeBotOS(api_key="your-api-key")

# 创建 Agent
agent = client.agents.create(
    name="MyAgent",
    capabilities=["L3_NetworkOut"]
)

# 与 Agent 对话
response = client.agents.chat(
    agent_id=agent.id,
    message="Hello!"
)

print(response.data.response)
```

### cURL

```bash
# 创建 Agent
curl -X POST https://api.beebotos.io/v1/agents \
  -H "X-API-Key: your-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "MyAgent",
    "capabilities": ["L3_NetworkOut"]
  }'

# 与 Agent 对话
curl -X POST https://api.beebotos.io/v1/agents/{agent_id}/message \
  -H "X-API-Key: your-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "message": "Hello!"
  }'
```

---

## 速率限制

| 端点类型 | 限制 | 窗口 |
|---------|------|------|
| 认证 | 10 次/分钟 | 滑动窗口 |
| 读操作 | 1000 次/分钟 | 滑动窗口 |
| 写操作 | 100 次/分钟 | 滑动窗口 |
| WebSocket | 100 条/分钟 | 滑动窗口 |

超出限制将返回 `429 Too Many Requests`。

---

## 版本历史

| 版本 | 日期 | 更新内容 |
|------|------|---------|
| v1.0 | 2026-03-13 | 当前版本，包含所有新特性 |
| v1.5 | 2025-12-01 | 新增 A2A 协议支持 |
| v1.0 | 2025-09-01 | 初始版本 |

---

## 支持和反馈

- 📧 邮件: api@beebotos.io
- 💬 Discord: [开发者频道](https://discord.gg/beebotos)
- 🐛 问题报告: [GitHub Issues](https://github.com/linc77/beebotos/issues)

---

**文档版本**: v1.0.0  
**最后更新**: 2026-03-13
