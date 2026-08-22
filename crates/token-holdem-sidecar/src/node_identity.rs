use anyhow::{Context, Result};
use libp2p::identity::Keypair;
use rand_core::{OsRng, RngCore};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const MAX_NODE_KEY_BYTES: usize = 4 * 1_024;

pub(crate) fn load_or_create(path: &Path) -> Result<Keypair> {
    if path.exists() {
        return read_key(path);
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).context("无法创建节点身份密钥目录")?;
    let keypair = Keypair::generate_ed25519();
    let encoded = keypair
        .to_protobuf_encoding()
        .context("无法编码 libp2p 节点身份")?;
    let temporary = temporary_path(parent);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .with_context(|| format!("无法创建临时节点身份密钥：{}", temporary.display()))?;
    file.write_all(&encoded).context("无法写入节点身份密钥")?;
    file.sync_all().context("无法同步节点身份密钥")?;
    drop(file);
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(keypair),
        Err(error) if path.exists() => {
            let _ = fs::remove_file(&temporary);
            read_key(path).with_context(|| format!("并发创建节点身份时发生冲突：{error}"))
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error).context("无法提交节点身份密钥")
        }
    }
}

fn read_key(path: &Path) -> Result<Keypair> {
    let encoded =
        fs::read(path).with_context(|| format!("无法读取节点身份密钥：{}", path.display()))?;
    if encoded.is_empty() || encoded.len() > MAX_NODE_KEY_BYTES {
        anyhow::bail!("节点身份密钥长度无效：{}", path.display())
    }
    Keypair::from_protobuf_encoding(&encoded).context("节点身份密钥编码无效")
}

fn temporary_path(parent: &Path) -> PathBuf {
    let mut nonce = [0_u8; 8];
    OsRng.fill_bytes(&mut nonce);
    parent.join(format!(".libp2p-key-{}.tmp", hex::encode(nonce)))
}
