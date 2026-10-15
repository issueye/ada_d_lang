//! 网络运行时（M9，对应决策 D14）。
//!
//! **为什么要有独立驱动线程**：Tokio 需要一个被驱动的 reactor，而 VM 的字节码执行必须
//! 保持单线程与确定性（D2 的核心诉求）。所以这里把两者分开：
//!
//! ```text
//!   VM 线程（唯一执行字节码）            ada-net-driver 线程
//!   ─────────────────────────           ────────────────────────────
//!   submit(op) ──spawn────────────────► current-thread Tokio runtime
//!   try_take() ◄──完成队列─────────────  socket / TLS / HTTP
//!   wait_until(deadline) 阻塞等待
//! ```
//!
//! 网络完成事件**不参与**任务调度顺序：VM 只在自己空闲等待时才会收取完成事件，因此
//! `spawn`/`await` 的交错顺序仍由 VM 的 FIFO 就绪队列决定（`async.rs` 的确定性用例
//! 因此不受影响）。

use crate::error::NetError;
use crate::exec;
use crate::op::{NetOp, NetReply, OpId};
use crate::policy::NetPolicy;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// 完成事件队列：驱动线程放入，VM 线程取出。
#[derive(Default)]
struct CompletionQueue {
    queue: Mutex<VecDeque<(OpId, Result<NetReply, NetError>)>>,
    available: Condvar,
}

impl CompletionQueue {
    fn push(&self, op: OpId, result: Result<NetReply, NetError>) {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.push_back((op, result));
        self.available.notify_all();
    }

    /// 非阻塞取一个完成事件。
    fn try_pop(&self) -> Option<(OpId, Result<NetReply, NetError>)> {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.pop_front()
    }

    /// 取走指定 op 的完成事件，**其它事件留在队列里**（供 `run_blocking` 使用）。
    fn try_take_matching(&self, op: OpId) -> Option<Result<NetReply, NetError>> {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        let index = queue.iter().position(|(id, _)| *id == op)?;
        queue.remove(index).map(|(_, result)| result)
    }

    /// 阻塞等到 `deadline`（`None` = 无限等待），返回是否有事件可取。
    fn wait_until(&self, deadline: Option<Instant>) {
        let queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        if !queue.is_empty() {
            return;
        }
        match deadline {
            None => {
                let _unused = self
                    .available
                    .wait_timeout(queue, Duration::from_secs(3600))
                    .map(|(guard, _)| guard);
            }
            Some(deadline) => {
                let now = Instant::now();
                if deadline > now {
                    let _unused = self
                        .available
                        .wait_timeout(queue, deadline - now)
                        .map(|(guard, _)| guard);
                }
            }
        }
    }
}

/// 网络运行时句柄。**惰性创建**：没有网络调用时不启动驱动线程。
pub struct NetRuntime {
    policy: Arc<NetPolicy>,
    handle: tokio::runtime::Handle,
    state: exec::SharedState,
    completions: Arc<CompletionQueue>,
    next_op: AtomicU64,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    driver: Option<std::thread::JoinHandle<()>>,
}

impl NetRuntime {
    /// 启动驱动线程与 Tokio 运行时。
    pub fn new(policy: Arc<NetPolicy>) -> Result<Self, NetError> {
        let (handle_tx, handle_rx) = std::sync::mpsc::channel();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let driver = std::thread::Builder::new()
            .name("ada-net-driver".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(err) => {
                        let _ = handle_tx.send(Err(err.to_string()));
                        return;
                    }
                };
                if handle_tx.send(Ok(runtime.handle().clone())).is_err() {
                    return;
                }
                // 把运行时驱动起来，直到收到关闭信号。spawn 出去的任务都在这里推进。
                let _ = runtime.block_on(async move {
                    let _ = shutdown_rx.await;
                });
            })
            .map_err(NetError::from)?;

        let handle = handle_rx
            .recv()
            .map_err(|_| NetError::Io("network driver thread failed to start".to_string()))?
            .map_err(NetError::Io)?;

        Ok(Self {
            policy,
            handle,
            state: exec::SharedState::new(),
            completions: Arc::new(CompletionQueue::default()),
            next_op: AtomicU64::new(1),
            shutdown: Some(shutdown_tx),
            driver: Some(driver),
        })
    }

    pub fn policy(&self) -> &Arc<NetPolicy> {
        &self.policy
    }

    /// 提交一个操作，立即返回其 id（不阻塞）。
    pub fn submit(&self, op: NetOp) -> OpId {
        let op_id = self.next_op.fetch_add(1, Ordering::Relaxed);
        let completions = Arc::clone(&self.completions);
        let policy = Arc::clone(&self.policy);
        let state = self.state.clone();
        self.handle.spawn(async move {
            let result = exec::execute(op, policy, state).await;
            completions.push(op_id, result);
        });
        op_id
    }

    /// 非阻塞收取一个完成事件。
    pub fn try_take(&self) -> Option<(OpId, Result<NetReply, NetError>)> {
        self.completions.try_pop()
    }

    /// 队列里是否已有完成事件（不取出）。
    pub fn has_completions(&self) -> bool {
        self.completions
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
            .eq(&false)
    }

    /// 阻塞等待到 `deadline`（`None` = 等到有事件为止）。
    pub fn wait_until(&self, deadline: Option<Instant>) {
        self.completions.wait_until(deadline);
    }

    /// **降级路径**：同步跑完一个操作并返回结果。
    ///
    /// 用于无法挂起的上下文（迭代器适配器回调内，`rust_depth > 0`，见 SPEC_ASYNC §4.3）。
    /// 期间其它操作的完成事件会留在队列里，不会被丢弃。
    pub fn run_blocking(&self, op: NetOp) -> Result<NetReply, NetError> {
        let op_id = self.submit(op);
        let deadline = Instant::now() + self.policy.io_timeout() + self.policy.connect_timeout();
        loop {
            if let Some(result) = self.completions.try_take_matching(op_id) {
                return result;
            }
            if Instant::now() >= deadline {
                return Err(NetError::Timeout(self.policy.io_timeout()));
            }
            self.completions.wait_until(Some(deadline));
        }
    }
}

impl Drop for NetRuntime {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(driver) = self.driver.take() {
            let _ = driver.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::NetOp;

    fn loopback_policy() -> Arc<NetPolicy> {
        Arc::new(
            NetPolicy::new()
                .allow_hosts(["127.0.0.1"])
                .unwrap()
                .allow_private_addresses(true)
                .with_connect_timeout(Duration::from_secs(5))
                .with_io_timeout(Duration::from_secs(5)),
        )
    }

    /// 连接被白名单拒绝时，运行时把错误作为完成事件回投，而不是 panic。
    #[test]
    fn denied_host_is_reported_as_a_completion() {
        let runtime = NetRuntime::new(Arc::new(NetPolicy::new())).unwrap();
        let op_id = runtime.submit(NetOp::TcpConnect {
            host: "example.com".to_string(),
            port: 443,
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = loop {
            if let Some((id, result)) = runtime.try_take() {
                assert_eq!(id, op_id);
                break result;
            }
            if Instant::now() >= deadline {
                panic!("no completion within the deadline");
            }
            runtime.wait_until(Some(deadline));
        };
        assert!(matches!(result, Err(NetError::HostNotAllowed { .. })));
    }

    /// `run_blocking` 只取自己那个 op 的结果，其它完成事件留在队列里。
    #[test]
    fn run_blocking_leaves_other_completions_queued() {
        let runtime = NetRuntime::new(loopback_policy()).unwrap();
        // 先提交两个必定失败（端口不通）的操作
        let a = runtime.submit(NetOp::TcpConnect {
            host: "127.0.0.1".to_string(),
            port: 1,
        });
        let b = runtime.submit(NetOp::TcpConnect {
            host: "127.0.0.1".to_string(),
            port: 1,
        });
        // 阻塞等一个不存在的句柄：立即失败，但绝不能吞掉 a / b 的完成事件
        let _ = runtime.run_blocking(NetOp::TcpRead {
            handle: 999,
            max_bytes: 1,
        });

        // a / b 可能比 run_blocking 晚完成，所以要等它们（否则是在测时序而不是测行为）
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        while seen.len() < 2 && Instant::now() < deadline {
            match runtime.try_take() {
                Some((id, _)) => seen.push(id),
                None => runtime.wait_until(Some(deadline)),
            }
        }
        assert!(seen.contains(&a), "a 的完成事件不应被吞掉");
        assert!(seen.contains(&b), "b 的完成事件必须留在队列里");
    }
}
