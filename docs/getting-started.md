# 快速开始

> **5 分钟上手 BeeBotOS**

---

## 安装

### 使用 Docker (推荐)

```bash
docker pull beebotos/beebotos:latest
docker run -p 8080:8080 beebotos/beebotos:latest
```

### 从源码编译

```bash
git clone https://github.com/linc77/beebotos.git
cd beebotos
cargo build --release
./target/release/beebotos-server
```

## 创建第一个 Agent

```bash
# 使用 CLI
beebotos-cli agent create --name "MyAgent"

# 或使用 API
curl -X POST http://localhost:8080/v1/agents \
  -H "Content-Type: application/json" \
  -d '{"name": "MyAgent"}'
```

## 与 Agent 对话

```bash
# CLI
beebotos-cli agent chat agent_xxx --message "你好!"

# API
curl -X POST http://localhost:8080/v1/agents/agent_xxx/message \
  -H "Content-Type: application/json" \
  -d '{"message": "你好!"}'
```

## 下一步

- [完整教程](tutorials/01-quick-start.md)
- [创建第一个 Agent](tutorials/02-create-first-agent.md)
- [开发技能](tutorials/03-develop-skill.md)

---

**最后更新**: 2026-03-13
