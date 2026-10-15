//! CLI 的网络开关契约（M9 / T9.4）。
//!
//! 固定住三件事：
//! 1. **默认没有网络**——不给 `--net-allow` 时 `net`/`http` 不可用；
//! 2. 给了白名单才授予，且规则非法时是清晰的参数错误（而不是静默放行）；
//! 3. 环境变量不是提权通道（`--net-allow` 只能来自命令行）。
//!
//! 只测"拒绝/参数解析"这些不需要服务端的路径——需要真实往返的部分在
//! `crates/ada_engine/tests/net_sandbox.rs` 里用 loopback 服务端覆盖。

use std::process::Command;

fn run(args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ada_cli"))
        .args(args)
        .output()
        .expect("ada_cli should be runnable");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n"),
    )
}

#[test]
fn without_net_allow_the_network_modules_do_not_exist() {
    let (ok, _, stderr) = run(&["-e", "import net from \"std:net\"\nnet"]);
    assert!(!ok, "未授权时导入必须失败");
    assert!(
        stderr.contains("cannot import std module 'net'"),
        "stderr: {stderr}"
    );
}

#[test]
fn a_bare_network_name_reports_a_capability_denial() {
    let (ok, _, stderr) = run(&["-e", "http"]);
    assert!(!ok);
    assert!(
        stderr.contains("Capability") && stderr.contains("http"),
        "stderr: {stderr}"
    );
}

#[test]
fn granting_a_host_makes_the_modules_importable() {
    let (ok, stdout, stderr) = run(&[
        "--net-allow",
        "127.0.0.1",
        "--net-allow-private",
        "-e",
        "import net from \"std:net\"\ntype_of(net)",
    ]);
    assert!(ok, "给了白名单应当可导入；stderr: {stderr}");
    assert_eq!(stdout.trim(), "map");
}

#[test]
fn an_invalid_allow_rule_is_a_clear_argument_error() {
    let (ok, _, stderr) = run(&["--net-allow", "api.test:notaport", "-e", "1"]);
    assert!(!ok, "非法规则必须失败");
    assert!(
        stderr.contains("invalid --net-allow rule") && stderr.contains("invalid port"),
        "stderr: {stderr}"
    );
}

#[test]
fn a_host_outside_the_allowlist_is_refused_at_runtime() {
    let (ok, _, stderr) = run(&[
        "--net-allow",
        "127.0.0.1",
        "-e",
        "import net from \"std:net\"\nnet.tcp_connect(\"example.com\", 80)",
    ]);
    assert!(!ok, "白名单外的主机必须被拒绝");
    assert!(stderr.contains("not allowed"), "stderr: {stderr}");
}

#[test]
fn private_addresses_stay_blocked_until_explicitly_allowed() {
    // 白名单放行 127.0.0.1，但没有 --net-allow-private：地址级检查必须兜住。
    let (ok, _, stderr) = run(&[
        "--net-allow",
        "127.0.0.1",
        "-e",
        "import net from \"std:net\"\nnet.tcp_connect(\"127.0.0.1\", 9)",
    ]);
    assert!(!ok, "私有地址默认必须被拒绝");
    assert!(
        stderr.contains("private or reserved"),
        "stderr: {stderr}"
    );
}
