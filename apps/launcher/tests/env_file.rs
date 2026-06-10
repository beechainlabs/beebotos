use beebotos_launcher::{load_env_config, render_env_config, write_env_file, EnvConfig};

#[test]
fn load_env_config_reads_known_keys_only() {
    let config = load_env_config(
        r#"
# user note
UNKNOWN=value
DOUBAO_API_KEY=text-key
IMAGE_GENERATION_API_KEY=image-key
VIDEO_GENERATION_API_KEY=video-key
BEE_ALLOW_NETWORK=0
"#,
    );

    assert_eq!(config.text_model_key, "text-key");
    assert_eq!(config.image_generation_key, "image-key");
    assert_eq!(config.video_generation_key, "video-key");
}

#[test]
fn render_env_config_preserves_comments_and_unknown_keys() {
    let rendered = render_env_config(
        r#"# keep comment
UNKNOWN=value
DOUBAO_API_KEY=old
IMAGE_GENERATION_API_KEY=old-image
BEE_ALLOW_NETWORK=0
"#,
        &EnvConfig {
            text_model_key: "new-text".into(),
            image_generation_key: "new-image".into(),
            video_generation_key: "new-video".into(),
        },
    );

    assert!(rendered.contains("# keep comment"));
    assert!(rendered.contains("UNKNOWN=value"));
    assert!(rendered.contains("DOUBAO_API_KEY=new-text"));
    assert!(rendered.contains("IMAGE_GENERATION_API_KEY=new-image"));
    assert!(rendered.contains("VIDEO_GENERATION_API_KEY=new-video"));
    assert!(rendered.contains("BEE_ALLOW_NETWORK=1"));
    assert!(!rendered.contains("old"));
}

#[test]
fn render_env_config_clears_blank_values_without_leaking_old_values() {
    let rendered = render_env_config(
        "EXTRA=1\nDOUBAO_API_KEY=old-secret\nVIDEO_GENERATION_API_KEY=old-video\n",
        &EnvConfig {
            text_model_key: String::new(),
            image_generation_key: "image-key".into(),
            video_generation_key: String::new(),
        },
    );

    assert!(rendered.contains("EXTRA=1"));
    assert!(rendered.contains("DOUBAO_API_KEY="));
    assert!(rendered.contains("IMAGE_GENERATION_API_KEY=image-key"));
    assert!(rendered.contains("VIDEO_GENERATION_API_KEY="));
    assert!(!rendered.contains("old-secret"));
    assert!(!rendered.contains("old-video"));
    assert!(rendered.ends_with('\n'));
}

#[test]
fn write_env_file_creates_missing_parent_and_file() {
    let dir = tempfile::tempdir().unwrap();
    let env_path = dir.path().join("nested").join(".env");

    write_env_file(
        &env_path,
        &EnvConfig {
            text_model_key: "text".into(),
            image_generation_key: String::new(),
            video_generation_key: "video".into(),
        },
    )
    .unwrap();

    let contents = std::fs::read_to_string(env_path).unwrap();
    assert!(contents.contains("DOUBAO_API_KEY=text"));
    assert!(contents.contains("IMAGE_GENERATION_API_KEY="));
    assert!(contents.contains("VIDEO_GENERATION_API_KEY=video"));
    assert!(contents.contains("BEE_ALLOW_NETWORK=1"));
}
