# Tutorial 1: Getting Started with BeeBotOS

## Introduction

Welcome to BeeBotOS! This tutorial will guide you through the basics of setting up and using the operating system.

## Prerequisites

- Rust nightly toolchain
- Basic understanding of Rust
- (Optional) Docker for containerized deployment

## Step 1: Installation

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup install nightly
rustup default nightly
```

### Clone BeeBotOS

```bash
git clone https://github.com/linc77/beebotos.git
cd beebotos
```

### Build the Project

```bash
make build
```

## Step 2: Your First Agent

Let's create a simple agent:

```rust
use beebot_agents::{AgentRuntime, AgentConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create runtime
    let mut runtime = AgentRuntime::new(Default::default());
    
    // Configure agent
    let config = AgentConfig {
        name: "MyFirstAgent".to_string(),
        capabilities: vec!["compute".to_string()],
        ..Default::default()
    };
    
    // Spawn agent
    let agent_id = runtime.spawn(config).await?;
    println!("Created agent: {}", agent_id);
    
    // Clean up
    runtime.terminate(&agent_id).await?;
    
    Ok(())
}
```

Save this as `first_agent.rs` and run:

```bash
cargo run --example first_agent
```

## Step 3: Understanding the Architecture

BeeBotOS consists of several layers:

1. **Kernel** - Core OS functionality
2. **Social Brain** - AI and cognition
3. **Agents** - Runtime and communication
4. **Chain** - Blockchain integration
5. **Apps** - User interfaces

## Step 4: Using the CLI

The CLI tool provides easy access to all features:

```bash
# Start the kernel
beebot start

# Spawn an agent
beebot spawn MyAgent --capabilities compute,network

# List agents
beebot list

# Check status
beebot status
```

## Step 5: API Usage

The Gateway API provides HTTP access:

```bash
# Health check
curl http://localhost:8080/health

# List agents
curl http://localhost:8080/api/v1/agents

# Create agent
curl -X POST http://localhost:8080/api/v1/agents \
  -H "Content-Type: application/json" \
  -d '{"name": "APIAgent", "capabilities": ["compute"]}'
```

## Next Steps

- Tutorial 2: Creating Skills
- Tutorial 3: Agent Communication
- Tutorial 4: DAO Participation
- Tutorial 5: Advanced Configuration

## Getting Help

- Join our [Discord](https://discord.gg/beebotos)
- Check the [FAQ](../FAQ.md)
- Read the [API docs](../docs/api.md)
