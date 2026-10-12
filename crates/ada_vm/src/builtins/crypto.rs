//! `crypto` 模块（T7.5）：纯计算的哈希原语。
//!
//! 与 `fs` 不同，这里不涉及宿主能力（不读环境、不碰网络、不落盘），因此
//! **三档 profile 都注册**，和 `json`/`math` 一样属于"纯计算模块"。
//!
//! 提供：
//!
//! | 函数 | 说明 |
//! | :--- | :--- |
//! | `crypto.sha256(text)` | SHA-256，返回 64 位十六进制小写字符串 |
//! | `crypto.hmac_sha256(key, text)` | HMAC-SHA256（键与消息都按 UTF-8 字节处理），返回十六进制 |
//! | `crypto.hex(text)` | 把字符串的 UTF-8 字节编码成十六进制（便于拼接/落盘） |
//!
//! 实现使用 `sha2`/`hmac` crate（而不是自己写密码学原语）；正确性由
//! `crates/ada_engine/tests/builtins_crypto.rs` 用已知向量与独立实现（Node.js
//! `crypto`）交叉验证。

use crate::value::Value;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub fn create_crypto_module() -> Value {
    let mut module = HashMap::new();

    module.insert(
        "sha256".to_string(),
        Value::native_fn("crypto.sha256", |args| {
            let text = text_arg(args, "crypto.sha256")?;
            Ok(Value::string(hex(&Sha256::digest(text.as_bytes()))))
        }),
    );

    module.insert(
        "hmac_sha256".to_string(),
        Value::native_fn("crypto.hmac_sha256", |args| {
            let key = args
                .first()
                .ok_or_else(|| "crypto.hmac_sha256 expects (key, text)".to_string())?
                .to_string();
            let text = args
                .get(1)
                .ok_or_else(|| "crypto.hmac_sha256 expects (key, text)".to_string())?
                .to_string();

            // Hmac::new_from_slice 接受任意长度密钥（HMAC 规范允许），仅当类型
            // 不匹配时才失败，而我们的键始终是字节切片，因此不会是错误路径。
            let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes())
                .map_err(|err| format!("crypto.hmac_sha256: {err}"))?;
            mac.update(text.as_bytes());
            Ok(Value::string(hex(&mac.finalize().into_bytes())))
        }),
    );

    module.insert(
        "hex".to_string(),
        Value::native_fn("crypto.hex", |args| {
            let text = text_arg(args, "crypto.hex")?;
            Ok(Value::string(hex(text.as_bytes())))
        }),
    );

    Value::map(module)
}

fn text_arg(args: &[Value], who: &str) -> Result<String, String> {
    match args.first() {
        Some(value) if !value.is_nil() => Ok(value.to_string()),
        _ => Err(format!("{who} expects a string argument")),
    }
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encoding_is_lowercase_and_padded() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        assert_eq!(hex(b""), "");
    }

    #[test]
    fn sha256_matches_known_vectors() {
        // FIPS 180-4 / 常见测试向量
        assert_eq!(
            hex(&Sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
