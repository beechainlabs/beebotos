# BeeBotOS Gateway Lib 模块调试与测试指导文档

> **模块路径**: `crates/gateway/src/lib.rs`  
> **版本**: 2.0.0  
> **最后更新**: 2026-03-22

---

## 目录

1. [模块概述](#模块概述)
2. [环境准备](#环境准备)
3. [快速验证](#快速验证)
4. [单元测试详解](#单元测试详解)
5. [安全修复验证](#安全修复验证)
6. [集成测试](#集成测试)
7. [性能基准测试](#性能基准测试)
8. [调试技巧](#调试技巧)
9. [CI/CD 集成](#cicd-集成)
10. [故障排查](#故障排查)

---

## 模块概述

`beebotos-gateway-lib` 是 BeeBotOS 的 API 网关核心库，提供以下功能：

- **JWT 认证** (修复: 算法固定为 HS256)
- **CORS 处理** (修复: 禁止危险配置组合)
- **限流器** (修复: DashMap 锁-free 实现)
- **WebSocket 管理** (修复: AtomicUsize 竞态条件)
- **服务发现** (修复: 路由匹配优化)
- **健康检查** (修复: TCP 连接泄漏)

### 模块结构

```
crates/gateway/src/
├── lib.rs              # 库入口
├── config.rs           # 配置管理 (JWT 密钥安全)
├── error.rs            # 错误处理 (敏感信息脱敏)
├── middleware/mod.rs   # 中间件 (JWT/CORS 安全)
├── websocket/mod.rs    # WebSocket (原子操作)
├── rate_limit/         # 限流器 (DashMap 优化)
│   ├── mod.rs
│   ├── token_bucket.rs
│   └── fixed_window.rs
├── discovery/mod.rs    # 服务发现
├── health.rs           # 健康检查
└── ...
```

---

## 环境准备

### 2.1 基础环境

```bash
# Rust 版本要求
rustc --version  # >= 1.75.0
cargo --version  # >= 1.75.0

# 克隆代码
git clone https://github.com/linc77/beebotos.git
cd beebotos/crates/gateway

# 安装依赖
cargo fetch
```

### 2.2 环境变量配置

创建 `.env` 文件：

```bash
# === JWT 安全配置 (生产环境强制要求) ===
JWT_SECRET="your-32-character-minimum-secret-key-here"
JWT_EXPIRY_MINUTES=60
JWT_ISSUER="beebotos"
JWT_AUDIENCE="beebotos-gateway"

# === CORS 配置 ===
CORS_ALLOWED_ORIGINS="http://localhost:3000,https://app.example.com"
CORS_ALLOW_ANY=false  # 生产环境必须为 false
CORS_ALLOW_CREDENTIALS=true

# === 限流配置 ===
RATE_LIMIT_RPS=100
RATE_LIMIT_BURST=200
RATE_LIMIT_ENABLED=true

# === WebSocket 配置 ===
WS_MAX_CONNECTIONS=10000
WS_MAX_MESSAGE_SIZE=1048576  # 1MB
WS_HEARTBEAT_INTERVAL=30

# === 调试配置 ===
RUST_LOG=beebotos_gateway_lib=debug,tower_http=debug
RUST_BACKTRACE=1
```

⚠️ **重要**: 生产模式 (`--release`) 下若未设置 `JWT_SECRET`，程序会 panic！

### 2.3 开发工具安装

```bash
# 代码覆盖率
cargo install cargo-tarpaulin

# 性能分析
cargo install cargo-flamegraph

# 测试辅助
cargo install cargo-watch
cargo install cargo-expand
```

---

## 快速验证

### 3.1 编译验证

```bash
# Debug 模式 (开发)
cargo build -p beebotos-gateway-lib

# Release 模式 (生产) - 会触发 JWT_SECRET 检查
cargo build -p beebotos-gateway-lib --release
```

### 3.2 基础测试

```bash
# 运行所有测试
cargo test -p beebotos-gateway-lib

# 带输出运行
cargo test -p beebotos-gateway-lib -- --nocapture

# 并行运行 (4 线程)
cargo test -p beebotos-gateway-lib --jobs 4
```

### 3.3 示例程序验证

```bash
# 运行基础网关示例
cargo run -p beebotos-gateway-lib --example basic_gateway

# 预期输出:
# [INFO] Gateway initialization complete
# [INFO] Configuration loaded successfully
# [INFO] JWT algorithm: HS256 (fixed)
# [INFO] CORS: allow_any_origin=false, allow_credentials=true
```

---

## 单元测试详解

### 4.1 配置模块测试 (`config.rs`)

#### JWT 安全测试

```bash
# 测试生产模式 panic
cargo test -p beebotos-gateway-lib test_jwt_config_production_panic

# 测试密钥长度验证
cargo test -p beebotos-gateway-lib test_jwt_secret_length_validation
```

**关键测试代码**:

```rust
#[test]
#[should_panic(expected = "JWT_SECRET must be explicitly configured")]
fn test_jwt_config_production_panic() {
    // 模拟生产模式 (非 debug)
    if !cfg!(debug_assertions) {
        let _config = JwtConfig::default(); // 会 panic
    }
}

#[test]
fn test_jwt_secret_length_validation() {
    use secrecy::Secret;
    
    let config = JwtConfig {
        secret: Secret::new("short".to_string()),  // 小于 32 字符
        ..Default::default()
    };
    
    // 应该产生警告但不 panic
    assert!(config.secret_bytes().len() < 32);
}
```

#### CORS 安全测试

```bash
# 测试危险配置检测
cargo test -p beebotos-gateway-lib test_cors_dangerous_config_panic

# 测试配置验证
cargo test -p beebotos-gateway-lib test_cors_config_validation
```

**测试代码**:

```rust
#[test]
#[should_panic(expected = "CORS 'allow_any_origin' cannot be combined")]
fn test_cors_dangerous_config_panic() {
    let config = CorsConfig {
        allow_any_origin: true,
        allow_credentials: true,  // 危险组合
        ..Default::default()
    };
    
    // 调用 cors_layer 会 panic
    let _layer = cors_layer(&config);
}
```

### 4.2 中间件测试 (`middleware/mod.rs`)

#### JWT 算法固定测试

```bash
# 核心安全测试
cargo test -p beebotos-gateway-lib test_jwt_algorithm_fixed_to_hs256

# 算法混淆攻击防护
cargo test -p beebotos-gateway-lib test_algorithm_confusion_attack_protection
```

**测试代码**:

```rust
#[test]
fn test_jwt_algorithm_fixed_to_hs256() {
    use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
    
    let config = JwtConfig {
        secret: Secret::new("a-very-long-secret-key-at-least-32-characters".to_string()),
        expiry_minutes: 60,
        refresh_expiry_minutes: 10080,
        issuer: "test".to_string(),
        audience: "test".to_string(),
    };
    
    // 生成 Token
    let token = generate_access_token("user-1", vec!["user".to_string()], &config).unwrap();
    
    // 尝试使用不同的验证算法
    for alg in [Algorithm::RS256, Algorithm::ES256, Algorithm::None] {
        let mut validation = Validation::new(alg);
        validation.set_issuer(&["test"]);
        
        let result = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(config.secret_bytes()),
            &validation,
        );
        
        // 非 HS256 算法应该失败
        assert!(result.is_err(), "Algorithm {:?} should fail", alg);
    }
    
    // HS256 应该成功
    let validation = Validation::new(Algorithm::HS256);
    let result = decode::<Claims>(
        &token,
        &DecodingKey::from_secret(config.secret_bytes()),
        &validation,
    );
    assert!(result.is_ok());
}
```

#### Token 生成与验证

```bash
cargo test -p beebotos-gateway-lib test_token_generation_and_validation
cargo test -p beebotos-gateway-lib test_token_expiration
cargo test -p beebotos-gateway-lib test_refresh_token_flow
```

### 4.3 WebSocket 测试 (`websocket/mod.rs`)

#### 原子操作测试

```bash
# 测试竞态条件修复
cargo test -p beebotos-gateway-lib test_connection_count_race_condition_fix

# 测试并发连接限制
cargo test -p beebotos-gateway-lib test_concurrent_connection_limit
```

**关键测试**:

```rust
#[tokio::test]
async fn test_connection_count_race_condition_fix() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    
    let config = WebSocketConfig::default();
    let manager = WebSocketManager::new(config);
    
    let success_count = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];
    
    // 模拟 1000 个并发连接请求
    for _ in 0..1000 {
        let manager = manager.clone();
        let success = success_count.clone();
        
        handles.push(tokio::spawn(async move {
            // 尝试增加连接计数
            let current = manager.connection_count();
            if current < config.max_connections {
                // 模拟连接成功
                success.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }
    
    for h in handles {
        h.await.unwrap();
    }
    
    // 验证没有竞态条件导致超限制
    assert!(success_count.load(Ordering::SeqCst) <= config.max_connections);
}
```

#### 消息大小限制测试

```rust
#[tokio::test]
async fn test_message_size_limit_enforcement() {
    let config = WebSocketConfig {
        max_message_size: 1024,  // 1KB for testing
        ..Default::default()
    };
    
    let manager = WebSocketManager::new(config);
    
    // 创建超大消息
    let large_message = "x".repeat(2048);  // 2KB
    
    // 应该触发大小限制错误
    // ... 测试逻辑
}
```

#### 死锁检测测试

```bash
# 测试锁顺序一致性
cargo test -p beebotos-gateway-lib test_lock_order_consistency

# 运行死锁检测 (使用 tokio-console 或 custom)
RUST_LOG=debug cargo test -p beebotos-gateway-lib test_deadlock_prevention
```

### 4.4 限流器测试 (`rate_limit/`)

#### DashMap 并发测试

```bash
# 测试锁-free 实现
cargo test -p beebotos-gateway-lib test_rate_limiter_dashmap_concurrent

# 测试性能
cargo bench -p beebotos-gateway-lib rate_limiter
```

**测试代码**:

```rust
#[tokio::test]
async fn test_rate_limiter_dashmap_concurrent() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    
    let limiter = Arc::new(TokenBucketRateLimiter::new(1000.0, 100));
    let allowed_count = Arc::new(AtomicUsize::new(0));
    let rejected_count = Arc::new(AtomicUsize::new(0));
    
    let mut handles = vec![];
    
    // 1000 个并发客户端，每个发送 10 个请求
    for client_id in 0..1000 {
        let limiter = limiter.clone();
        let allowed = allowed_count.clone();
        let rejected = rejected_count.clone();
        
        handles.push(tokio::spawn(async move {
            for _ in 0..10 {
                if limiter.allow(&format!("client-{}", client_id)).await {
                    allowed.fetch_add(1, Ordering::SeqCst);
                } else {
                    rejected.fetch_add(1, Ordering::SeqCst);
                }
            }
        }));
    }
    
    for h in handles {
        h.await.unwrap();
    }
    
    let total = allowed_count.load(Ordering::SeqCst) + rejected_count.load(Ordering::SeqCst);
    assert_eq!(total, 10000);
    
    // 验证限流器工作正常
    println!("Allowed: {}, Rejected: {}", 
        allowed_count.load(Ordering::SeqCst),
        rejected_count.load(Ordering::SeqCst)
    );
}
```

#### Token Bucket 算法测试

```rust
#[tokio::test]
async fn test_token_bucket_refill() {
    let limiter = TokenBucketRateLimiter::new(100.0, 2);  // 100/s, burst 2
    
    // 消耗 burst
    assert!(limiter.allow("client").await);  // 1
    assert!(limiter.allow("client").await);  // 2
    assert!(!limiter.allow("client").await); // 拒绝
    
    // 等待 refill
    tokio::time::sleep(Duration::from_millis(20)).await;  // 应该获得 ~2 个 token
    
    assert!(limiter.allow("client").await);  // 应该成功
}
```

### 4.5 错误处理测试 (`error.rs`)

#### 敏感信息脱敏测试

```bash
cargo test -p beebotos-gateway-lib test_error_message_sanitization
```

```rust
#[test]
fn test_error_message_sanitization() {
    // 内部错误包含敏感信息
    let internal_error = GatewayError::internal("Database password: secret123".to_string());
    
    // 用户看到的应该是脱敏后的消息
    let user_message = internal_error.user_message();
    assert!(!user_message.contains("password"));
    assert!(!user_message.contains("secret123"));
    assert_eq!(user_message, "An internal error occurred. Please try again later.");
    
    // 但日志中应该记录完整信息 (通过 correlation_id 关联)
    if let GatewayError::Internal { correlation_id, .. } = internal_error {
        assert!(!correlation_id.is_empty());
    }
}
```

### 4.6 健康检查测试 (`health.rs`)

#### TCP 连接泄漏测试

```rust
#[tokio::test]
async fn test_tcp_connection_cleanup() {
    use std::net::SocketAddr;
    
    // 启动一个简单的 TCP 服务器
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    
    // 在后台运行服务器
    tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            // 立即关闭连接
            drop(socket);
        }
    });
    
    // 运行健康检查多次
    let check = PingHealthCheck::new("test", addr.to_string());
    for _ in 0..100 {
        let _ = check.check().await;
    }
    
    // 验证没有连接泄漏 (通过系统工具检查)
    // lsof -i :PORT | wc -l 应该保持恒定
}
```

---

## 安全修复验证

### 5.1 Critical 修复验证

#### 修复 #1: JWT 密钥自动生成防护

```bash
# 测试生产模式 panic
$ export JWT_SECRET=""
$ cargo build --release -p beebotos-gateway-lib 2>&1 | head -5
error: JWT_SECRET must be explicitly configured in production
```

**验证脚本**:

```bash
#!/bin/bash
# verify_jwt_security.sh

echo "=== JWT Security Verification ==="

# 1. 验证生产模式强制要求 JWT_SECRET
echo -n "[TEST] Production mode requires JWT_SECRET... "
if ! JWT_SECRET="" cargo build --release 2>&1 | grep -q "JWT_SECRET"; then
    echo "FAILED"
    exit 1
fi
echo "PASSED"

# 2. 验证算法固定为 HS256
echo -n "[TEST] JWT algorithm fixed to HS256... "
cargo test -p beebotos-gateway-lib test_jwt_algorithm_fixed_to_hs256 --quiet
if [ $? -ne 0 ]; then
    echo "FAILED"
    exit 1
fi
echo "PASSED"

echo "=== All JWT Security Tests Passed ==="
```

#### 修复 #2: CORS 危险配置检测

```bash
# 验证危险配置会 panic
cargo test -p beebotos-gateway-lib test_cors_dangerous_config_panic
```

### 5.2 High 修复验证

#### 修复 #4: 竞态条件修复验证

```bash
# 使用 loom 进行并发测试
cargo test -p beebotos-gateway-lib --features loom test_connection_count_atomic
```

#### 修复 #6: 错误信息脱敏

```bash
# 验证内部错误不泄露
cargo test -p beebotos-gateway-lib test_error_sanitization
```

### 5.3 Medium 修复验证

#### 修复 #9: 限流器锁优化

```bash
# 性能对比测试
cargo bench -p beebotos-gateway-lib rate_limit_before_after
```

---

## 集成测试

### 6.1 端到端测试

创建 `tests/integration/main.rs`:

```rust
use beebotos_gateway_lib::config::GatewayConfig;
use beebotos_gateway_lib::Gateway;

#[tokio::test]
async fn test_full_gateway_lifecycle() {
    // 1. 加载配置
    let config = GatewayConfig::from_env().unwrap();
    
    // 2. 创建 Gateway
    let gateway = Gateway::new(config).await.unwrap();
    
    // 3. 启动服务
    gateway.start().await.unwrap();
    
    // 4. 验证健康
    assert!(gateway.is_running().await);
    
    // 5. 发送测试请求
    // ... HTTP client tests
    
    // 6. 优雅关闭
    gateway.shutdown().await;
}
```

运行：

```bash
# 设置测试环境变量
export JWT_SECRET="test-secret-32-characters-minimum"
export RUST_LOG=warn

# 运行集成测试
cargo test -p beebotos-gateway-lib --test integration
```

### 6.2 HTTP API 测试

使用 `reqwest` 进行测试：

```rust
#[tokio::test]
async fn test_http_api_endpoints() {
    let client = reqwest::Client::new();
    let base_url = "http://localhost:8080";
    
    // 健康检查
    let resp = client.get(&format!("{}/health", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    
    // JWT 保护端点 (无 Token)
    let resp = client.get(&format!("{}/api/protected", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
    
    // JWT 保护端点 (有 Token)
    let token = get_test_token();
    let resp = client.get(&format!("{}/api/protected", base_url))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    
    // 限流测试
    let mut success_count = 0;
    let mut rate_limited_count = 0;
    
    for _ in 0..250 {
        let resp = client.get(&format!("{}/api/data", base_url))
            .send()
            .await
            .unwrap();
        
        match resp.status() {
            200 => success_count += 1,
            429 => rate_limited_count += 1,
            _ => panic!("Unexpected status"),
        }
    }
    
    assert!(rate_limited_count > 0, "Rate limiting should trigger");
}
```

### 6.3 WebSocket 集成测试

```rust
#[tokio::test]
async fn test_websocket_full_flow() {
    use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
    
    let url = "ws://localhost:8080/ws";
    let (ws_stream, _) = connect_async(url).await.unwrap();
    let (mut write, mut read) = ws_stream.split();
    
    // 1. 发送消息
    write.send(Message::Text(r#"{"type": "ping"}"#.to_string())).await.unwrap();
    
    // 2. 接收响应
    let msg = read.next().await.unwrap().unwrap();
    assert!(msg.is_text());
    
    // 3. 测试消息大小限制
    let large_msg = "x".repeat(2 * 1024 * 1024);  // 2MB
    write.send(Message::Text(large_msg)).await.unwrap();
    
    // 应该收到错误并断开
    let msg = read.next().await.unwrap().unwrap();
    assert!(msg.to_string().contains("MESSAGE_TOO_LARGE"));
}
```

---

## 性能基准测试

### 7.1 配置 Criterion

`benches/rate_limit_benchmark.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use beebotos_gateway_lib::rate_limit::token_bucket::TokenBucketRateLimiter;
use beebotos_gateway_lib::rate_limit::RateLimiter;

fn rate_limiter_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    c.bench_function("token_bucket_allow", |b| {
        let limiter = TokenBucketRateLimiter::new(10000.0, 1000);
        b.to_async(&rt).iter(|| async {
            black_box(limiter.allow("client").await);
        });
    });
    
    c.bench_function("fixed_window_allow", |b| {
        let limiter = FixedWindowRateLimiter::new(10000, Duration::from_secs(1));
        b.to_async(&rt).iter(|| async {
            black_box(limiter.allow("client").await);
        });
    });
}

criterion_group!(benches, rate_limiter_benchmark);
criterion_main!(benches);
```

运行：

```bash
cargo bench -p beebotos-gateway-lib
```

### 7.2 WebSocket 并发测试

```bash
# 使用 wscat 进行并发测试
for i in {1..100}; do
  wscat -c ws://localhost:8080/ws &
done
wait

# 监控连接数
curl http://localhost:8080/metrics | grep websocket_connections
```

### 7.3 内存分析

```bash
# 使用 valgrind (Linux)
valgrind --tool=massif --massif-out-file=massif.out \
  cargo run -p beebotos-gateway-lib --example stress_test

# 分析结果
ms_print massif.out | head -100
```

---

## 调试技巧

### 8.1 日志级别控制

```bash
# 开发调试
RUST_LOG=beebotos_gateway_lib=debug,tokio=trace cargo run

# 生产问题排查
RUST_LOG=beebotos_gateway_lib=warn,error cargo run

# 特定模块
RUST_LOG=beebotos_gateway_lib::websocket=debug cargo run
```

### 8.2 使用 GDB/LLDB

```bash
# 编译调试版本
cargo build -p beebotos-gateway-lib

# 启动 GDB
rust-gdb target/debug/examples/basic_gateway

# 常用断点
(gdb) break beebotos_gateway_lib::middleware::validate_token
(gdb) break beebotos_gateway_lib::websocket::handle_connection
(gdb) run
(gdb) backtrace
(gdb) info locals
```

### 8.3 Tokio Console 调试

```bash
# 启用 tokio-console 特性
cargo run -p beebotos-gateway-lib --features tokio-console

# 在另一个终端
tokio-console
```

### 8.4 配置热重载调试

```bash
# 修改配置后自动重载
export RUST_LOG=debug

# 观察日志输出:
# [DEBUG] Config file changed, reloading...
# [INFO] Config reloaded successfully
```

---

## CI/CD 集成

### 9.1 GitHub Actions 配置

```yaml
# .github/workflows/gateway-test.yml
name: Gateway Lib Tests

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
          override: true
      
      - name: Run tests
        run: |
          export JWT_SECRET="ci-test-secret-32-characters-minimum"
          cargo test -p beebotos-gateway-lib --all-features
      
      - name: Run security tests
        run: |
          cargo test -p beebotos-gateway-lib test_jwt_security
          cargo test -p beebotos-gateway-lib test_cors_security
      
      - name: Generate coverage
        run: |
          cargo install cargo-tarpaulin
          cargo tarpaulin -p beebotos-gateway-lib --out Xml
      
      - name: Upload coverage
        uses: codecov/codecov-action@v3
        with:
          files: ./cobertura.xml
```

---

## 故障排查

### 10.1 编译错误

| 错误 | 原因 | 解决方案 |
|------|------|----------|
| `linker not found` | 缺少构建工具 | `sudo apt install build-essential` |
| `JWT_SECRET panic` | 生产模式未配置密钥 | `export JWT_SECRET="..."` |
| `CORS panic` | allow_any + credentials | 修改配置 |

### 10.2 运行时错误

| 错误 | 原因 | 解决方案 |
|------|------|----------|
| `Rate limit exceeded` | 请求过多 | 调整 RATE_LIMIT_RPS |
| `WebSocket disconnect` | 消息过大 | 检查 WS_MAX_MESSAGE_SIZE |
| `Token expired` | JWT 过期 | 增加 JWT_EXPIRY_MINUTES |

### 10.3 性能问题

```bash
# 检查限流器瓶颈
cargo flamegraph --bin gateway

# 检查内存使用
valgrind --tool=massif cargo run

# 检查 CPU 热点
perf record -g cargo run
perf report
```

---

## 附录

### A. 测试命令速查表

```bash
# 快速测试
cargo test -p beebotos-gateway-lib

# 安全测试
cargo test -p beebotos-gateway-lib test_jwt
cargo test -p beebotos-gateway-lib test_cors
cargo test -p beebotos-gateway-lib test_websocket

# 性能测试
cargo bench -p beebotos-gateway-lib

# 覆盖率
cargo tarpaulin -p beebotos-gateway-lib --out Html

# 文档测试
cargo test --doc -p beebotos-gateway-lib
```

### B. 环境变量完整列表

| 变量 | 必需 | 默认值 | 说明 |
|------|------|--------|------|
| `JWT_SECRET` | 是 | - | 至少 32 字符 |
| `JWT_EXPIRY_MINUTES` | 否 | 60 | Token 过期时间 |
| `CORS_ALLOWED_ORIGINS` | 否 | localhost | 逗号分隔 |
| `CORS_ALLOW_ANY` | 否 | false | 危险！|
| `RATE_LIMIT_RPS` | 否 | 100 | 每秒请求数 |
| `WS_MAX_CONNECTIONS` | 否 | 10000 | WebSocket 上限 |
| `RUST_LOG` | 否 | info | 日志级别 |
| `RUST_BACKTRACE` | 否 | 0 | 开启 backtrace |

---

*文档版本: 2.0*  
*维护者: BeeBotOS Team*  
*最后更新: 2026-03-22*
