//! 自愈层：重试 + 指数退避、熔断器、多后端级联 fallback。
//!
//! 这是"自愈"的一等公民实现：任何会失败的操作（LLM 调用、工具执行）都包在这层里，
//! 失败时自动重试、熔断不健康后端、级联到下一个可用后端（lkgp 粘路径在外层 Gateway）。

use std::time::{Duration, Instant};

/// 熔断器状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed, // 正常放行
    Open,   // 熔断中，拒绝放行
    HalfOpen, // 冷却结束，允许一次探针
}

#[derive(Debug)]
pub struct CircuitBreaker {
    threshold: u32,
    cooldown: Duration,
    failures: u32,
    opened_at: Option<Instant>,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, cooldown: Duration) -> Self {
        Self {
            threshold: threshold.max(1),
            cooldown,
            failures: 0,
            opened_at: None,
        }
    }

    pub fn state(&self) -> BreakerState {
        if self.failures < self.threshold {
            return BreakerState::Closed;
        }
        match self.opened_at {
            Some(t) if t.elapsed() >= self.cooldown => BreakerState::HalfOpen,
            Some(_) => BreakerState::Open,
            None => BreakerState::Closed,
        }
    }

    pub fn allow(&self) -> bool {
        self.state() != BreakerState::Open
    }

    pub fn record_success(&mut self) {
        self.failures = 0;
        self.opened_at = None;
    }

    pub fn record_failure(&mut self) {
        self.failures += 1;
        if self.failures >= self.threshold {
            self.opened_at = Some(Instant::now());
        }
    }
}

/// 通用自愈（同步）：重试 + 指数退避。`E` 需可 `Debug` 以便返回末次错误。
pub fn with_retry<F, T, E>(mut f: F, attempts: u32, base: Duration) -> Result<T, E>
where
    F: FnMut() -> Result<T, E>,
    E: std::fmt::Debug,
{
    let attempts = attempts.max(1);
    let mut last: Option<E> = None;
    for i in 0..attempts {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                last = Some(e);
                if i + 1 < attempts {
                    std::thread::sleep(base * 2u32.pow(i));
                }
            }
        }
    }
    Err(last.unwrap())
}

/// 通用自愈（异步）：重试 + 指数退避，不阻塞线程。
pub async fn with_retry_async<F, Fut, T, E>(mut f: F, attempts: u32, base: Duration) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Debug,
{
    let attempts = attempts.max(1);
    let mut last: Option<E> = None;
    for i in 0..attempts {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                last = Some(e);
                if i + 1 < attempts {
                    tokio::time::sleep(base * 2u32.pow(i)).await;
                }
            }
        }
    }
    Err(last.unwrap())
}

/// 多后端级联 fallback（同步）：依次尝试，返回首个 `Ok`，否则收集全部错误。
pub fn fallback_chain<F, T, E>(steps: &[F]) -> Result<T, Vec<E>>
where
    F: Fn() -> Result<T, E>,
{
    let mut errs = Vec::new();
    for s in steps {
        match s() {
            Ok(v) => return Ok(v),
            Err(e) => errs.push(e),
        }
    }
    Err(errs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_succeeds_after_failures() {
        let mut n = 0;
        let r = with_retry(
            || {
                n += 1;
                if n < 3 {
                    Err("boom")
                } else {
                    Ok("ok")
                }
            },
            5,
            Duration::from_millis(1),
        );
        assert_eq!(r, Ok("ok"));
        assert_eq!(n, 3);
    }

    #[test]
    fn breaker_opens_then_recovers() {
        let mut b = CircuitBreaker::new(2, Duration::from_millis(10));
        assert!(b.allow());
        b.record_failure();
        assert!(b.allow());
        b.record_failure(); // 达到阈值，打开
        assert!(!b.allow());
        std::thread::sleep(Duration::from_millis(15));
        assert!(b.allow()); // 冷却结束，半开探针
        b.record_success();
        assert!(b.allow());
    }

    #[test]
    fn fallback_chain_returns_first_ok() {
        let r = fallback_chain(&[
            || -> Result<&str, &str> { Err("a") },
            || -> Result<&str, &str> { Ok("b") },
            || -> Result<&str, &str> { Ok("c") },
        ]);
        assert_eq!(r, Ok("b"));
    }
}
