use std::path::Path;
use std::{fs, io};

pub const TEXT_MODEL_KEY: &str = "DOUBAO_API_KEY";
pub const IMAGE_GENERATION_KEY: &str = "IMAGE_GENERATION_API_KEY";
pub const VIDEO_GENERATION_KEY: &str = "VIDEO_GENERATION_API_KEY";
pub const ALLOW_NETWORK_KEY: &str = "BEE_ALLOW_NETWORK";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EnvConfig {
    pub text_model_key: String,
    pub image_generation_key: String,
    pub video_generation_key: String,
}

pub fn load_env_config(contents: &str) -> EnvConfig {
    let mut config = EnvConfig::default();
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let value = value.trim().to_owned();
        match key.trim() {
            TEXT_MODEL_KEY => config.text_model_key = value,
            IMAGE_GENERATION_KEY => config.image_generation_key = value,
            VIDEO_GENERATION_KEY => config.video_generation_key = value,
            _ => {}
        }
    }
    config
}

pub fn render_env_config(existing: &str, config: &EnvConfig) -> String {
    let mut output = Vec::new();

    for line in existing.lines() {
        let trimmed = line.trim_start();
        let key = trimmed
            .split_once('=')
            .map(|(key, _)| key.trim())
            .unwrap_or_default();
        if matches!(
            key,
            TEXT_MODEL_KEY | IMAGE_GENERATION_KEY | VIDEO_GENERATION_KEY | ALLOW_NETWORK_KEY
        ) {
            continue;
        }
        output.push(line.to_owned());
    }

    push_key(&mut output, TEXT_MODEL_KEY, &config.text_model_key);
    push_key(
        &mut output,
        IMAGE_GENERATION_KEY,
        &config.image_generation_key,
    );
    push_key(
        &mut output,
        VIDEO_GENERATION_KEY,
        &config.video_generation_key,
    );
    output.push(format!("{ALLOW_NETWORK_KEY}=1"));

    let mut rendered = output.join("\n");
    rendered.push('\n');
    rendered
}

pub fn read_env_file(path: &Path) -> io::Result<EnvConfig> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(load_env_config(&contents)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(EnvConfig::default()),
        Err(err) => Err(err),
    }
}

pub fn write_env_file(path: &Path, config: &EnvConfig) -> io::Result<()> {
    let existing = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err),
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, render_env_config(&existing, config))
}

fn push_key(output: &mut Vec<String>, key: &str, value: &str) {
    output.push(format!("{key}={}", value.trim()));
}

#[cfg(windows)]
pub mod windows;
