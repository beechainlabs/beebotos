# BeeBotOS SDK

SDK for developing skills (plugins) for BeeBotOS agents.

## Quick Start

```bash
cargo add beebot-sdk
```

```rust
use beebot_sdk::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub api_key: String,
}

pub struct MySkill {
    config: Config,
}

impl Skill for MySkill {
    type Config = Config;
    
    fn new(config: Config) -> Result<Self, SkillError> {
        Ok(Self { config })
    }
    
    async fn handle_message(
        &self,
        ctx: &Context,
        msg: Message,
    ) -> Result<Message, SkillError> {
        ctx.log("Processing message...").await?;
        
        Ok(Message::response(&msg, serde_json::json!({
            "status": "ok"
        })))
    }
}

beebot_sdk::export_skill!(MySkill);
```

## Building

```bash
# Build for WASM
cargo build --target wasm32-wasi --release

# The output is in target/wasm32-wasi/release/*.wasm
```

## Testing

```bash
# Unit tests
cargo test

# Integration tests
cargo test --features integration
```

## Documentation

- [Full Documentation](https://docs.beebotos.io/sdk)
- [Examples](https://github.com/linc77/beebotos/tree/main/examples)
- [API Reference](https://docs.rs/beebot-sdk)

## License

MIT
