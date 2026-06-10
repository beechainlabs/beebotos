# 🧠 beebotos-social-brain 模块调试与测试指南

## 📋 目录

1. [环境准备](#环境准备)
2. [快速开始](#快速开始)
3. [单元测试](#单元测试)
4. [基准测试](#基准测试)
5. [集成测试](#集成测试)
6. [调试技巧](#调试技巧)
7. [常见问题](#常见问题)
8. [性能分析](#性能分析)
9. [持续集成](#持续集成)

---

## 环境准备

### 1.1 基础环境要求

```bash
# Rust 工具链 (最低版本 1.70)
rustup --version

# 必需的组件
rustup component add rustfmt clippy llvm-tools-preview

# 测试工具
cargo install cargo-nextest cargo-llvm-cov cargo-audit

# 基准测试工具
cargo install cargo-criterion
```

### 1.2 项目配置

```bash
# 克隆项目
git clone https://github.com/linc77/beebotos.git
cd beebotos

# 检查 social-brain 模块
cd crates/social-brain

# 验证依赖
cargo tree | grep social-brain
```

### 1.3 IDE 配置 (VS Code)

```json
// .vscode/settings.json
{
  "rust-analyzer.cargo.features": "all",
  "rust-analyzer.checkOnSave.command": "clippy",
  "rust-analyzer.checkOnSave.extraArgs": ["--all-targets"],
  "rust-analyzer.cargo.unsetTest": ["core"]
}
```

---

## 快速开始

### 2.1 编译验证

```bash
# 检查代码 (不生成可执行文件)
cargo check -p beebotos-social-brain

# 详细检查 (包含测试)
cargo check --all-targets -p beebotos-social-brain

# 发布模式编译
cargo build --release -p beebotos-social-brain
```

### 2.2 运行所有测试

```bash
# 运行全部测试
cargo test -p beebotos-social-brain

# 带输出的详细测试
RUST_LOG=debug cargo test -p beebotos-social-brain -- --nocapture

# 使用 nextest (推荐)
cargo nextest run -p beebotos-social-brain
```

### 2.3 代码质量检查

```bash
# 格式化检查
cargo fmt -p beebotos-social-brain -- --check

# 自动格式化
cargo fmt -p beebotos-social-brain

# Clippy 静态分析
cargo clippy -p beebotos-social-brain --all-targets -- -D warnings

# 安全审计
cargo audit -p beebotos-social-brain
```

---

## 单元测试

### 3.1 测试文件结构

```
crates/social-brain/
├── src/
│   └── lib.rs                 # 模块内联测试
├── tests/
│   ├── neat_test.rs          # NEAT 进化测试 (15个测试)
│   ├── memory_test.rs        # 记忆系统测试 (28个测试)
│   ├── pad_test.rs           # PAD 情绪测试 (25个测试)
│   ├── cognitive_test.rs     # 认知功能测试 (27个测试)
│   └── cognition_tests.rs    # 综合认知测试 (30个测试)
└── benches/                  # 基准测试
    ├── pad_bench.rs
    ├── neat_evolution_bench.rs
    ├── memory_query_bench.rs
    └── reasoning_bench.rs
```

### 3.2 NEAT 模块测试

```bash
# 运行 NEAT 全部测试
cargo test -p beebotos-social-brain neat

# 运行特定测试
cargo test -p beebotos-social-brain test_genome_crossover
cargo test -p beebotos-social-brain test_population_evolution

# 详细输出
RUST_LOG=debug cargo test -p beebotos-social-brain neat -- --nocapture
```

**关键测试用例说明：**

| 测试名称 | 功能 | 调试重点 |
|---------|------|---------|
| `test_genome_creation` | 基因组基础创建 | 验证层结构 |
| `test_genome_crossover` | 基因交叉 | 检查基因继承 |
| `test_structural_mutation_add_node` | 添加节点变异 | 创新号追踪 |
| `test_structural_mutation_add_connection` | 添加连接变异 | 连接有效性 |
| `test_neural_network_activation` | 前向传播 | 数值范围检查 |
| `test_population_evolution` | 完整进化流程 | 适应度计算 |
| `test_compatibility_distance` | 兼容性距离 | 物种形成 |

**调试示例：**

```rust
#[test]
fn debug_genome_mutation() {
    let mut genome = Genome::new_minimal(1, 3, 2);
    println!("Before mutation:");
    println!("  Layers: {:?}", genome.layers);
    println!("  Connections: {}", genome.connections.len());
    
    let config = NeatConfig::aggressive();
    let mut innovations = InnovationTracker::new();
    genome.mutate(&config, &mut innovations);
    
    println!("After mutation:");
    println!("  Connections: {}", genome.connections.len());
    println!("  Enabled: {}", 
        genome.connections.iter().filter(|c| c.enabled).count()
    );
    
    // 验证变异合理性
    assert!(genome.connections.len() >= 6);
}
```

### 3.3 记忆系统测试

```bash
# 运行记忆系统测试
cargo test -p beebotos-social-brain memory

# 特定子模块
cargo test -p beebotos-social-brain short_term
cargo test -p beebotos-social-brain episodic
cargo test -p beebotos-social-brain semantic
```

**测试分类：**

```rust
// 短期记忆测试
#[test]
fn test_short_term_memory_capacity() {
    let mut stm = ShortTermMemory::with_capacity(3);
    
    // 填充到容量上限
    stm.push("item1");
    stm.push("item2");
    stm.push("item3");
    assert_eq!(stm.len(), 3);
    
    // 添加第四个，应该驱逐一个
    let evicted = stm.push("item4");
    assert!(evicted.is_some());
    assert_eq!(stm.len(), 3);
    
    // 调试输出
    println!("Evicted: {:?}", evicted);
    println!("Remaining: {:?}", stm.items());
}

// 情节记忆测试
#[test]
fn test_episodic_memory_queries() {
    let mut em = EpisodicMemory::new();
    
    // 添加测试数据
    em.encode("Morning meeting", 100, Some(Location::new("Office")));
    em.encode("Lunch break", 500, Some(Location::new("Cafe")));
    em.encode("Evening workout", 1000, None);
    
    // 时间范围查询
    let results = em.query_time_range(200, 800);
    println!("Time query results: {:?}", results);
    assert_eq!(results.len(), 1);
    
    // 位置查询
    let office_events = em.query_location("Office");
    println!("Location query results: {:?}", office_events);
}

// 记忆巩固测试
#[test]
fn test_memory_consolidation_flow() {
    let mut memory = UnifiedMemory::new();
    
    // 添加STM项目并重复激活
    memory.short_term.push_with_priority("important fact", Priority::High);
    let id = memory.short_term.items()[0].id.clone();
    
    // 模拟多次重复
    for i in 0..5 {
        let result = memory.short_term.rehearse(&id);
        println!("Rehearsal {}: {:?}", i, result);
    }
    
    // 执行巩固
    let consolidated = memory.consolidate().unwrap();
    println!("Consolidated {} memories", consolidated);
    
    assert_eq!(consolidated, 1);
}
```

### 3.4 PAD 情绪模型测试

```bash
# PAD 情绪测试
cargo test -p beebotos-social-brain pad
cargo test -p beebotos-social-brain emotion

# 特定情绪转换测试
cargo test -p beebotos-social-brain test_pad_to_basic_emotion_conversion
```

**调试情绪状态：**

```rust
#[test]
fn debug_emotional_transitions() {
    let mut ei = EmotionalIntelligence::new();
    
    // 初始状态
    println!("Baseline: {:?}", ei.current());
    
    // 正向刺激
    ei.update(&EmotionalEvent {
        description: "Success".to_string(),
        pleasure_impact: 0.6,
        arousal_impact: 0.3,
        dominance_impact: 0.2,
    });
    println!("After positive: {:?}", ei.current());
    
    // 负向刺激
    ei.update(&EmotionalEvent {
        description: "Failure".to_string(),
        pleasure_impact: -0.5,
        arousal_impact: 0.4,
        dominance_impact: -0.2,
    });
    println!("After negative: {:?}", ei.current());
    
    // 情绪衰减
    for i in 0..5 {
        ei.tick();
        println!("Tick {}: {:?}", i, ei.current());
    }
    
    // 验证边界
    let pad = ei.current();
    assert!(pad.pleasure >= -1.0 && pad.pleasure <= 1.0);
    assert!(pad.arousal >= 0.0 && pad.arousal <= 1.0);
    assert!(pad.dominance >= 0.0 && pad.dominance <= 1.0);
}
```

### 3.5 认知模块测试

```bash
# 认知功能测试
cargo test -p beebotos-social-brain cognition
cargo test -p beebotos-social-brain cognitive

# 特定功能
cargo test -p beebotos-social-brain goal
cargo test -p beebotos-social-brain working_memory
```

**认知状态调试：**

```rust
#[test]
fn debug_cognitive_state() {
    let mut state = CognitiveState::new();
    
    // 添加工作记忆项目
    state.memorize(MemoryItem {
        key: "task".to_string(),
        value: json!("complete documentation"),
        activation: 0.8,
        timestamp: 1234567890,
    });
    
    // 设置多个目标
    state.set_goal(Goal::new("Write tests", 0.9));
    state.set_goal(Goal::new("Fix bugs", 0.7));
    state.set_goal(Goal::new("Refactor", 0.5));
    
    // 验证优先级排序
    println!("Goals (should be sorted by priority):");
    for (i, goal) in state.goals.iter().enumerate() {
        println!("  {}: {} (priority: {})", i, goal.description, goal.priority);
    }
    
    // 形成意图
    if let Some(intention) = state.form_intention() {
        println!("Formed intention for goal: {}", intention.goal_id);
    }
    
    // 工作记忆衰减
    println!("Before decay: {:?}", state.working_memory.items());
    state.working_memory.decay();
    println!("After decay: {:?}", state.working_memory.items());
}
```

---

## 基准测试

### 4.1 运行基准测试

```bash
# 运行所有基准
cargo bench -p beebotos-social-brain

# 特定基准
cargo bench -p beebotos-social-brain -- pad
cargo bench -p beebotos-social-brain -- neat

# 保存基准结果
cargo bench -p beebotos-social-brain -- --save-baseline initial
```

### 4.2 基准测试报告

运行后查看报告：

```bash
# 打开 HTML 报告
open target/criterion/pad_operations/report/index.html

# 基准比较
cargo bench -p beebotos-social-brain -- --baseline initial
```

### 4.3 关键性能指标

| 模块 | 测试项 | 目标性能 |
|-----|-------|---------|
| PAD | 单次操作 | < 1 μs |
| PAD | 批量 1000 次 | < 1 ms |
| NEAT | 网络激活 (100 节点) | < 100 μs |
| NEAT | 一代进化 (150 个体) | < 100 ms |
| Memory | 查询 (1000 条) | < 10 ms |
| Memory | 巩固 | < 50 ms |

---

## 集成测试

### 5.1 端到端测试

```bash
# 运行示例作为集成测试
cargo run --example evolve_agent -p beebotos-social-brain

# 验证输出
RUST_LOG=info cargo run --example evolve_agent -p beebotos-social-brain 2>&1 | tee evolve.log
```

### 5.2 集成测试示例

```rust
// tests/integration_test.rs
use beebotos_social_brain::{
    SocialBrainApi, BrainConfig,
    pad::Pad,
    memory::MemoryQuery,
};

#[test]
fn test_full_cognitive_loop() {
    // 初始化 API
    let mut api = SocialBrainApi::new();
    
    // 1. 处理刺激
    let response = api.process_stimulus("Urgent: system failure!").unwrap();
    assert!(!response.memory_id.is_empty());
    
    // 2. 验证情绪变化
    let emotion = api.current_pad();
    println!("Emotion after urgent: {:?}", emotion);
    assert!(emotion.arousal > 0.5, "Urgent message should increase arousal");
    
    // 3. 设置目标
    let goal_id = api.set_goal("Fix the system", 0.95);
    assert!(!goal_id.is_empty());
    
    // 4. 查询记忆
    let query = MemoryQuery::new("system failure");
    let results = api.query_memory(&query).unwrap();
    println!("Memory query results: {} items", results.total_count());
    
    // 5. 巩固记忆
    let consolidated = api.consolidate_memories().unwrap();
    println!("Consolidated {} memories", consolidated);
    
    // 6. 验证统计
    let stats = api.stats();
    println!("API stats: {:?}", stats);
    assert!(stats.active_goals > 0);
}

#[test]
fn test_emotional_contagion_scenario() {
    use beebotos_social_brain::pad::EmotionalIntelligence;
    
    // 模拟两个 Agent
    let mut agent1 = EmotionalIntelligence::new();
    let mut agent2 = EmotionalIntelligence::new();
    
    // Agent1 经历负面事件
    agent1.update(&EmotionalEvent {
        description: "Lost data".to_string(),
        pleasure_impact: -0.7,
        arousal_impact: 0.5,
        dominance_impact: -0.3,
    });
    
    println!("Agent1 emotion: {:?}", agent1.current());
    
    // Agent2 共情
    agent2.empathize(agent1.current());
    println!("Agent2 after empathy: {:?}", agent2.current());
    
    // 验证情绪传播
    assert!(agent2.current().pleasure < 0.0, 
        "Agent2 should feel negative after empathy");
}
```

### 5.3 压力测试

```rust
#[test]
fn test_memory_pressure() {
    use beebotos_social_brain::memory::ShortTermMemory;
    
    let mut stm = ShortTermMemory::with_capacity(100);
    
    // 快速添加大量项目
    for i in 0..1000 {
        stm.push(&format!("item_{}", i));
    }
    
    // 验证容量限制
    assert!(stm.len() <= 100);
    
    // 验证高优先级保留
    stm.push_with_priority("critical", Priority::Critical);
    let items: Vec<_> = stm.items().iter().map(|i| i.content.clone()).collect();
    assert!(items.contains(&"critical".to_string()));
}

#[test]
fn test_neat_large_population() {
    use beebotos_social_brain::neat::{Population, NeatConfig};
    
    let config = NeatConfig::standard();
    let mut population = Population::new(500, 10, 5, &config);
    
    // 运行多代
    for generation in 0..10 {
        // 生成随机适应度
        let fitness_results: Vec<_> = population.genomes.iter().enumerate()
            .map(|(i, g)| FitnessResult {
                agent_id: AgentId::new(&format!("agent_{}", g.id)),
                fitness: rand::random::<f32>(),
                generation,
                metrics: HashMap::new(),
            })
            .collect();
        
        population.evolve(&fitness_results, &config);
        
        let stats = population.stats();
        println!("Gen {}: {} species, best={:.2}", 
            generation, stats.species_count, stats.best_fitness);
    }
    
    // 验证物种形成
    assert!(population.species.len() > 0);
}
```

---

## 调试技巧

### 6.1 日志记录

```rust
use tracing::{debug, info, warn, error, span, Level};

#[test]
fn test_with_logging() {
    // 初始化日志 (在测试开始时调用一次)
    let _ = tracing_subscriber::fmt::try_init();
    
    let span = span!(Level::INFO, "neat_test", generation = 1);
    let _enter = span.enter();
    
    info!("Starting NEAT evolution test");
    
    let config = NeatConfig::standard();
    debug!(?config, "Using config");
    
    let mut population = Population::new(50, 3, 2, &config);
    
    for gen in 0..5 {
        info!(generation = gen, "Evolving...");
        
        // 模拟某些条件
        if gen == 3 {
            warn!(generation = gen, "Low diversity detected");
        }
        
        // 进化逻辑...
    }
    
    info!("Test completed");
}
```

**运行时日志级别：**

```bash
# 错误级别
RUST_LOG=error cargo test -p beebotos-social-brain

# 警告级别
RUST_LOG=warn cargo test -p beebotos-social-brain

# 信息级别 (推荐)
RUST_LOG=info cargo test -p beebotos-social-brain -- --nocapture

# 调试级别 (详细)
RUST_LOG=debug cargo test -p beebotos-social-brain neat -- --nocapture

# 追踪级别 (最详细)
RUST_LOG=trace cargo test -p beebotos-social-brain

# 按模块过滤
RUST_LOG=beebotos_social_brain::neat=debug,beebotos_social_brain::memory=info
```

### 6.2 断点调试

**VS Code 配置：**

```json
// .vscode/launch.json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "Debug NEAT Test",
      "cargo": {
        "args": ["test", "--no-run", "--package", "beebotos-social-brain"],
        "filter": {
          "name": "beebotos-social-brain",
          "kind": "lib"
        }
      },
      "args": ["test_genome_crossover", "--nocapture"],
      "cwd": "${workspaceFolder}"
    }
  ]
}
```

**关键断点位置：**

| 文件 | 函数 | 说明 |
|-----|------|------|
| `neat/genome.rs` | `crossover()` | 基因交叉点 |
| `neat/mod.rs` | `evolve()` | 进化代 |
| `memory/mod.rs` | `consolidate()` | 记忆巩固 |
| `pad/emotion.rs` | `update()` | 情绪更新 |
| `api.rs` | `process_stimulus()` | 刺激处理 |

### 6.3 状态检查宏

```rust
// 自定义调试宏
#[macro_export]
macro_rules! debug_genome {
    ($genome:expr) => {
        println!("Genome #{}:", $genome.id);
        println!("  Layers: {}", $genome.layers.len());
        println!("  Nodes: {}", $genome.node_count());
        println!("  Connections: {} (enabled: {})", 
            $genome.connections.len(),
            $genome.connections.iter().filter(|c| c.enabled).count()
        );
        println!("  Fitness: {:.4}", $genome.fitness);
    };
}

#[macro_export]
macro_rules! assert_pad_in_bounds {
    ($pad:expr) => {
        assert!($pad.pleasure >= -1.0 && $pad.pleasure <= 1.0,
            "Pleasure out of bounds: {}", $pad.pleasure);
        assert!($pad.arousal >= 0.0 && $pad.arousal <= 1.0,
            "Arousal out of bounds: {}", $pad.arousal);
        assert!($pad.dominance >= 0.0 && $pad.dominance <= 1.0,
            "Dominance out of bounds: {}", $pad.dominance);
    };
}

// 使用示例
#[test]
fn test_with_debug_macros() {
    let genome = Genome::new_minimal(1, 3, 2);
    debug_genome!(genome);
    
    let pad = Pad::new(0.5, 0.6, 0.7);
    assert_pad_in_bounds!(pad);
}
```

### 6.4 内存调试

```bash
# 使用 Miri 检测未定义行为
cargo +nightly miri test -p beebotos-social-brain

# 内存泄漏检测
cargo test -p beebotos-social-brain --features leak_detection

# 地址消毒器 (Linux)
RUSTFLAGS="-Z sanitizer=address" cargo +nightly test -p beebotos-social-brain
```

---

## 常见问题

### 7.1 编译问题

**问题：找不到 crate**
```
error: no matching package named `beebotos-core` found
```

解决：
```bash
# 在 workspace 根目录运行
cargo build -p beebotos-social-brain

# 或者先构建依赖
cargo build -p beebotos-core
cargo build -p beebotos-social-brain
```

**问题：特性未启用**
```
error: cannot find derive macro `Serialize` in this scope
```

解决：
```toml
# 检查 Cargo.toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }
```

### 7.2 测试失败

**问题：浮点数比较失败**
```
assertion failed: pad.pleasure == 0.3
```

解决：
```rust
// 使用近似比较
assert!((pad.pleasure - 0.3).abs() < 1e-6);

// 或使用 approx crate
use approx::assert_relative_eq;
assert_relative_eq!(pad.pleasure, 0.3, epsilon = 1e-6);
```

**问题：随机测试不稳定**
```rust
// 设置固定种子
use rand::SeedableRng;
use rand::rngs::StdRng;

#[test]
fn deterministic_test() {
    let mut rng = StdRng::seed_from_u64(42);
    // 使用 rng 代替 rand::random
}
```

### 7.3 性能问题

**问题：测试运行缓慢**

```bash
# 使用 release 模式运行测试
cargo test -p beebotos-social-brain --release

# 只运行特定测试
cargo test -p beebotos-social-brain test_population_evolution

# 并行运行测试
cargo test -p beebotos-social-brain -- --test-threads=8
```

**问题：基准测试波动大**

```rust
// 增加样本量
#[bench]
fn stable_benchmark(b: &mut Bencher) {
    b.iter_batched(
        || setup(),
        |data| process(data),
        BatchSize::SmallInput,
    );
}
```

### 7.4 调试内存泄漏

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

static INSTANCE_COUNT: AtomicUsize = AtomicUsize::new(0);

struct TrackedResource;

impl TrackedResource {
    fn new() -> Self {
        INSTANCE_COUNT.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for TrackedResource {
    fn drop(&mut self) {
        INSTANCE_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

#[test]
fn check_memory_leaks() {
    let initial = INSTANCE_COUNT.load(Ordering::SeqCst);
    
    {
        let _resources: Vec<_> = (0..100).map(|_| TrackedResource::new()).collect();
    } // resources dropped here
    
    let final_count = INSTANCE_COUNT.load(Ordering::SeqCst);
    assert_eq!(initial, final_count, "Memory leak detected!");
}
```

---

## 性能分析

### 8.1 使用 flamegraph

```bash
# 安装
cargo install flamegraph

# 生成火焰图
cargo flamegraph --package beebotos-social-brain --test neat_test

# 查看
open flamegraph.svg
```

### 8.2 使用 perf (Linux)

```bash
# 编译带调试信息的 release
cargo build --release -p beebotos-social-brain

# 记录性能数据
perf record --call-graph dwarf target/release/deps/neat_test-*

# 生成报告
perf report

# 火焰图
perf script | stackcollapse-perf.pl | flamegraph.pl > perf.svg
```

### 8.3 代码覆盖率

```bash
# 使用 cargo-llvm-cov
cargo llvm-cov -p beebotos-social-brain

# 生成 HTML 报告
cargo llvm-cov -p beebotos-social-brain --html --open

# 仅覆盖特定模块
cargo llvm-cov -p beebotos-social-brain --package beebotos-social-brain

# 与 CI 集成
cargo llvm-cov -p beebotos-social-brain --lcov --output-path lcov.info
```

### 8.4 性能回归测试

```rust
// benches/regression_bench.rs
use criterion::{criterion_group, criterion_main, Criterion};

fn evolution_regression(c: &mut Criterion) {
    let mut group = c.benchmark_group("regression");
    
    // 与之前版本比较
    group.bench_function("evolution_v1", |b| {
        b.iter(|| {
            // 新版本实现
        })
    });
    
    group.finish();
}

criterion_group!(benches, evolution_regression);
criterion_main!(benches);
```

---

## 持续集成

### 9.1 GitHub Actions 配置

```yaml
# .github/workflows/social-brain.yml
name: Social Brain CI

on:
  push:
    paths:
      - 'crates/social-brain/**'
      - 'crates/core/**'
  pull_request:
    paths:
      - 'crates/social-brain/**'

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
    - uses: actions/checkout@v3
    
    - name: Setup Rust
      uses: dtolnay/rust-action@stable
      with:
        components: rustfmt, clippy
    
    - name: Cache dependencies
      uses: Swatinem/rust-cache@v2
      with:
        workspaces: "crates/social-brain"
    
    - name: Check formatting
      run: cargo fmt -p beebotos-social-brain -- --check
    
    - name: Run clippy
      run: cargo clippy -p beebotos-social-brain --all-targets -- -D warnings
    
    - name: Run tests
      run: cargo test -p beebotos-social-brain --all-features
    
    - name: Run doc tests
      run: cargo test -p beebotos-social-brain --doc
    
    - name: Generate coverage
      run: |
        cargo install cargo-llvm-cov
        cargo llvm-cov -p beebotos-social-brain --lcov --output-path lcov.info
    
    - name: Upload coverage
      uses: codecov/codecov-action@v3
      with:
        files: lcov.info

  benchmark:
    runs-on: ubuntu-latest
    if: github.event_name == 'pull_request'
    steps:
    - uses: actions/checkout@v3
    
    - name: Run benchmarks
      run: cargo bench -p beebotos-social-brain -- --baseline main
```

### 9.2 本地预提交检查

```bash
#!/bin/bash
# scripts/pre-commit.sh

set -e

echo "Running pre-commit checks..."

# 格式化
cargo fmt -p beebotos-social-brain -- --check

# Clippy
cargo clippy -p beebotos-social-brain --all-targets -- -D warnings

# 测试
cargo test -p beebotos-social-brain

# 文档测试
cargo test -p beebotos-social-brain --doc

# 基准测试 (快速模式)
cargo bench -p beebotos-social-brain -- --test

echo "All checks passed!"
```

---

## 附录

### A. 调试检查清单

- [ ] 代码格式化通过 `cargo fmt`
- [ ] Clippy 无警告 `cargo clippy`
- [ ] 单元测试通过 `cargo test`
- [ ] 文档测试通过 `cargo test --doc`
- [ ] 基准测试无性能回归 `cargo bench`
- [ ] 代码覆盖率 > 80%
- [ ] 内存检查无泄漏
- [ ] Miri 检查通过 (unsafe 代码)

### B. 快速命令参考

```bash
# 完整测试流程
cd crates/social-brain
cargo fmt -- --check && \
cargo clippy --all-targets -- -D warnings && \
cargo test --all-features && \
cargo test --doc && \
cargo bench -- --test && \
echo "All checks passed!"

# 调试特定测试
RUST_LOG=debug cargo test test_name -- --nocapture

# 性能分析
cargo flamegraph --test test_name

# 覆盖率
cargo llvm-cov --html --open
```

### C. 相关资源

- [Rust Testing Guide](https://doc.rust-lang.org/rustc/tests/index.html)
- [Criterion.rs Docs](https://bheisler.github.io/criterion.rs/book/)
- [Tracing Documentation](https://docs.rs/tracing/latest/tracing/)
- [Miri Undefined Behavior Detection](https://github.com/rust-lang/miri)

---

**文档版本**: v1.0  
**最后更新**: 2026-03-23  
**维护者**: BeeBotOS Team

%%%%%%%%%%%%%%%%%%%%%%%%%%%%%
我已为 beebotos-social-brain 模块创建了详细的调试和测试指导文档。

---

## 📄 文档已生成

**文件位置**: `docs/social-brain-debugging-testing-guide.md`

---

## 📋 文档内容概览

### 1. 环境准备
- Rust 工具链要求 (≥ 1.70)
- 必需组件安装 (`rustfmt`, `clippy`, `llvm-tools`)
- 测试工具安装 (`cargo-nextest`, `cargo-llvm-cov`, `cargo-audit`)
- VS Code IDE 配置

### 2. 快速开始
```bash
# 编译验证
cargo check -p beebotos-social-brain

# 运行测试
cargo test -p beebotos-social-brain

# 代码质量检查
cargo fmt -p beebotos-social-brain -- --check
cargo clippy -p beebotos-social-brain --all-targets -- -D warnings
```

### 3. 单元测试详解
- **125+ 测试用例**覆盖所有模块
- 各模块测试运行命令
- 关键测试用例说明表
- 调试示例代码

| 模块 | 测试数量 | 运行命令 |
|------|---------|---------|
| NEAT | 15 | `cargo test neat` |
| Memory | 28 | `cargo test memory` |
| PAD | 25 | `cargo test pad` |
| Cognition | 27 | `cargo test cognition` |

### 4. 基准测试
```bash
# 运行所有基准
cargo bench -p beebotos-social-brain

# 查看 HTML 报告
open target/criterion/pad_operations/report/index.html
```

### 5. 调试技巧
- **日志记录**: `RUST_LOG=debug cargo test -- --nocapture`
- **断点调试**: VS Code launch.json 配置
- **自定义调试宏**: `debug_genome!`, `assert_pad_in_bounds!`
- **内存调试**: `cargo miri test`

### 6. 常见问题解决
- 编译问题 (找不到 crate、特性未启用)
- 测试失败 (浮点数比较、随机测试不稳定)
- 性能问题 (测试缓慢、基准波动)
- 内存泄漏检测

### 7. 性能分析
- 火焰图生成 (`cargo flamegraph`)
- perf 性能分析
- 代码覆盖率 (`cargo llvm-cov`)
- 性能回归测试

### 8. CI/CD 配置
- GitHub Actions 完整配置
- 本地预提交检查脚本

---

## 🚀 快速命令速查

```bash
# 完整检查流程
cd crates/social-brain
cargo fmt -- --check && \
cargo clippy --all-targets -- -D warnings && \
cargo test --all-features && \
cargo test --doc && \
cargo bench -- --test

# 调试特定测试（带详细日志）
RUST_LOG=debug cargo test test_genome_crossover -- --nocapture

# 生成覆盖率报告
cargo llvm-cov -p beebotos-social-brain --html --open

# 性能火焰图
cargo flamegraph --package beebotos-social-brain --test neat_test
```

---

文档包含完整的示例代码、命令参考和故障排查指南，可直接用于日常开发和 CI/CD 流程。

