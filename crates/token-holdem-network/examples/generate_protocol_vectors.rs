#[path = "../tests/support/protocol_vectors.rs"]
mod protocol_vectors;

use protocol_vectors::build_protocol_vectors;
use std::{fs, io, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = output_path();
    let parent = output
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "固定向量输出目录无效"))?;
    fs::create_dir_all(parent)?;
    let mut json = serde_json::to_string_pretty(&build_protocol_vectors())?;
    json.push('\n');
    fs::write(&output, json)?;
    println!("已生成协议固定向量：{}", output.display());
    Ok(())
}

fn output_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test-vectors/protocol-9/core.json")
}
