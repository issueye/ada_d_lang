//! `crypto` 模块（开发计划 T7.5）：正确性交叉验证。
//!
//! 这些期望值**不是**由本仓库的实现生成的，而是用 Node.js 的 `crypto` 模块独立算出来的
//! （`createHash("sha256")` / `createHmac("sha256", ...)`），因此能真正发现实现错误：
//!
//! ```text
//! sha256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
//! sha256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
//! sha256("Ada-D 语言") = 35119bd7345e466b89c952cb83fa0073bca643ea33b5c8691b63ed27c6f91a02
//! hmac_sha256("key", "The quick brown fox…") = f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8
//! hmac_sha256("secret", "") = f9e66e179b6747ae54108f82f8ade8b3c25d76fd30afde6c395822c530196169
//! hmac_sha256("密钥", "消息") = 51785c6051c3d60392c441ada800662e64a027761ff96d71237c51da0eb25020
//! ```
//!
//! 另外验证：crypto 是**纯计算模块**，三档 profile 都可用（不需要宿主授权）。

use ada_engine::{Engine, Sandbox, Value};

#[test]
fn sha256_matches_an_independent_implementation() {
    let mut engine = Engine::new();
    assert_eq!(
        engine
            .eval::<String>("import crypto from \"std:crypto\"\ncrypto.sha256(\"\")")
            .unwrap(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        engine
            .eval::<String>("import crypto from \"std:crypto\"\ncrypto.sha256(\"abc\")")
            .unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    // 多字节：必须按 UTF-8 字节哈希（而不是按字符/UTF-16）
    assert_eq!(
        engine
            .eval::<String>("import crypto from \"std:crypto\"\ncrypto.sha256(\"Ada-D 语言\")")
            .unwrap(),
        "35119bd7345e466b89c952cb83fa0073bca643ea33b5c8691b63ed27c6f91a02"
    );
    assert_eq!(
        engine
            .eval::<String>("import crypto from \"std:crypto\"\ncrypto.sha256(\"The quick brown fox jumps over the lazy dog\")")
            .unwrap(),
        "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
    );
}

#[test]
fn hmac_sha256_matches_an_independent_implementation() {
    let mut engine = Engine::new();
    assert_eq!(
        engine
            .eval::<String>(
                "import crypto from \"std:crypto\"\ncrypto.hmac_sha256(\"key\", \"The quick brown fox jumps over the lazy dog\")"
            )
            .unwrap(),
        "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
    );
    // 空消息
    assert_eq!(
        engine
            .eval::<String>(
                "import crypto from \"std:crypto\"\ncrypto.hmac_sha256(\"secret\", \"\")"
            )
            .unwrap(),
        "f9e66e179b6747ae54108f82f8ade8b3c25d76fd30afde6c395822c530196169"
    );
    // 多字节键与消息
    assert_eq!(
        engine
            .eval::<String>(
                "import crypto from \"std:crypto\"\ncrypto.hmac_sha256(\"密钥\", \"消息\")"
            )
            .unwrap(),
        "51785c6051c3d60392c441ada800662e64a027761ff96d71237c51da0eb25020"
    );
}

#[test]
fn hex_encodes_utf8_bytes() {
    let mut engine = Engine::new();
    assert_eq!(
        engine
            .eval::<String>("import crypto from \"std:crypto\"\ncrypto.hex(\"Ada\")")
            .unwrap(),
        "416461"
    );
    assert_eq!(
        engine
            .eval::<String>("import crypto from \"std:crypto\"\ncrypto.hex(\"语言\")")
            .unwrap(),
        "e8afade8a880",
        "每个 UTF-8 字节两字符"
    );
    assert_eq!(
        engine
            .eval::<String>("import crypto from \"std:crypto\"\ncrypto.hex(\"\")")
            .unwrap(),
        ""
    );
}

#[test]
fn crypto_is_a_pure_computation_module_available_in_every_profile() {
    // 不涉及宿主能力：三档 profile 都注册（与 json/math 同级）
    for sandbox in [
        Sandbox::strict(),
        Sandbox::standard(),
        Sandbox::permissive(),
    ] {
        let mut engine = Engine::with_sandbox(sandbox.clone());
        assert_eq!(engine.eval::<String>("type_of(crypto)").unwrap(), "map");
        assert_eq!(
            engine
                .eval::<String>("import crypto from \"std:crypto\"\ncrypto.sha256(\"abc\")")
                .unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "profile {:?} 下也应可用",
            sandbox.kind
        );
    }
}

#[test]
fn argument_errors_are_native_errors_not_panics() {
    let mut engine = Engine::new();
    for source in [
        "import crypto from \"std:crypto\"\ncrypto.sha256()",
        "import crypto from \"std:crypto\"\ncrypto.sha256(nil)",
        "import crypto from \"std:crypto\"\ncrypto.hmac_sha256(\"key\")",
        "import crypto from \"std:crypto\"\ncrypto.hex()",
    ] {
        let err = engine.eval_value(source).expect_err("should fail");
        let text = err.to_string();
        assert!(
            text.contains("expects") || text.contains("Native"),
            "{source} produced an unexpected error: {text}"
        );
    }
}

#[test]
fn hashing_is_deterministic_and_length_stable() {
    let mut engine = Engine::new();
    let first = engine
        .eval::<String>("import crypto from \"std:crypto\"\ncrypto.sha256(\"repeatable\")")
        .unwrap();
    let second = engine
        .eval::<String>("import crypto from \"std:crypto\"\ncrypto.sha256(\"repeatable\")")
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.len(), 64, "SHA-256 十六进制应恒为 64 字符");
    assert_eq!(
        engine
            .eval::<Value>("import crypto from \"std:crypto\"\ncrypto.sha256(\"x\") |> len()")
            .unwrap(),
        Value::Int(64)
    );
}
