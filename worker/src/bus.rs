//! The line between the engine and the platform.
//!
//! The engine is ordinary Rust: it has futures, it awaits things, and it
//! knows nothing about D1, KV, R2 or `fetch`. What it cannot do is wait:
//! a Worker is single-threaded, and blocking on a promise would stop the
//! very event loop that would resolve it.
//!
//! So the engine asks instead. [`Bus::call`] hands the host a question and
//! returns a future that is not ready until the host has answered. The
//! engine suspends there; the host runs the question — a statement against
//! D1, a request to a provider, a read from the cache — and hands the
//! answer back; the engine continues where it left off.
//!
//! The questions and the answers are JSON, because the host is JavaScript
//! and JSON is what both sides already speak. Every question carries a
//! `kind`, and the answer to a question of kind `query` is a table:
//!
//! ```json
//! {"kind": "query", "sql": "SELECT 1", "params": [], "rows": true}
//! ```
//!
//! ```json
//! {"rows": [{"columns": ["n"], "values": [1]}], "changes": 0}
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use mosql::Value;

/// A future the engine hands back to the host.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The question the engine is asking.
#[derive(Debug)]
struct Question {
    /// Which question this is, so an answer cannot be given to the wrong one.
    ticket: u64,
    /// The question itself.
    op: Value,
    /// Whether the host has been given it yet.
    handed: bool,
}

#[derive(Debug, Default)]
struct State {
    /// The counter that names each question.
    next: u64,
    /// The question the engine is asking, if any.
    asked: Option<Question>,
    /// The answer the host gave.
    answered: Option<Value>,
}

/// The engine's side of the line.
///
/// Cloning it shares it: the store, the cache and the blob adapter all hold
/// the same bus, and they all go through it.
#[derive(Debug, Clone, Default)]
pub struct Bus {
    state: Arc<Mutex<State>>,
}

impl Bus {
    /// A bus nobody has spoken on yet.
    pub fn new() -> Bus {
        Bus::default()
    }

    /// Ask the host something, and wait for the answer.
    ///
    /// The engine stops here until the host answers, which is the whole
    /// point: the host is the only thing that can do I/O.
    pub async fn call(&self, op: Value) -> Result<Value, String> {
        let ticket = {
            let mut state = self.state.lock().expect("bus");
            state.next += 1;
            let ticket = state.next;
            state.asked = Some(Question {
                ticket,
                op,
                handed: false,
            });
            ticket
        };
        Wait {
            bus: self.clone(),
            ticket,
        }
        .await
    }

    /// The question the engine is asking, if the host has not had it yet.
    ///
    /// The host calls this after the engine has stopped, and runs what it
    /// finds. It is called once per question: a second call before the
    /// answer comes back returns nothing, because there is only one
    /// question in flight at a time.
    pub fn take_asked(&self) -> Option<Value> {
        let mut state = self.state.lock().expect("bus");
        let question = state.asked.as_mut()?;
        if question.handed {
            return None;
        }
        question.handed = true;
        Some(question.op.clone())
    }

    /// Hand the host's answer to the engine.
    pub fn deliver(&self, answer: Value) -> Result<(), String> {
        let mut state = self.state.lock().expect("bus");
        if state.asked.is_none() {
            return Err("the engine is not waiting for an answer".into());
        }
        state.answered = Some(answer);
        Ok(())
    }

    /// Whether the engine is waiting for something.
    pub fn is_waiting(&self) -> bool {
        self.state.lock().expect("bus").asked.is_some()
    }
}

/// The future the engine waits on.
struct Wait {
    bus: Bus,
    ticket: u64,
}

impl Future for Wait {
    type Output = Result<Value, String>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.bus.state.lock().expect("bus");
        let Some(question) = state.asked.as_ref() else {
            // The question was answered and taken; this poll is a stray.
            return Poll::Ready(Err("the engine is not waiting for an answer".into()));
        };
        if question.ticket != self.ticket {
            // An answer to a different question: not ours.
            return Poll::Pending;
        }
        match state.answered.take() {
            Some(answer) => {
                state.asked = None;
                Poll::Ready(Ok(answer))
            }
            None => Poll::Pending,
        }
    }
}

/// The engine's work, as a future the host can stop and restart.
pub type Root = Pin<Box<dyn Future<Output = Value> + Send>>;

/// What happened when the engine was polled.
pub enum Step {
    /// The engine finished; here is its answer.
    Done(Value),
    /// The engine is waiting; here is the question.
    Ask(Value),
    /// The engine stopped without asking anything. That is a bug: the only
    /// thing it can wait for is the host.
    Stalled,
}

/// Poll the engine once, and say what it wants.
///
/// The host calls this after every answer, and follows it with whatever
/// the engine asked for.
pub fn step(root: &mut Root, bus: &Bus) -> Step {
    // The engine is polled once, and then again when the host has an
    // answer for it. There is no other way it becomes ready, so the waker
    // has nothing to do.
    let mut cx = Context::from_waker(std::task::Waker::noop());
    match root.as_mut().poll(&mut cx) {
        Poll::Ready(value) => Step::Done(value),
        Poll::Pending => match bus.take_asked() {
            Some(op) => Step::Ask(op),
            None => Step::Stalled,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_is_handed_out_once() {
        let bus = Bus::new();
        let op = Value::str("hello");
        // Ask without awaiting: the question is registered when the future is
        // first polled, so drive it by hand.
        let mut future = Box::pin(bus.call(op.clone()));
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert!(matches!(future.as_mut().poll(&mut cx), Poll::Pending));
        assert_eq!(bus.take_asked(), Some(op.clone()));
        assert_eq!(bus.take_asked(), None, "the question is handed out once");
    }

    #[test]
    fn an_answer_reaches_the_waiting_future() {
        let bus = Bus::new();
        let mut future = Box::pin(bus.call(Value::str("hello")));
        let mut cx = Context::from_waker(std::task::Waker::noop());
        assert!(matches!(future.as_mut().poll(&mut cx), Poll::Pending));
        assert!(bus.is_waiting());
        bus.deliver(Value::str("answered")).expect("deliver");
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(Ok(value)) => assert_eq!(value, Value::str("answered")),
            other => panic!("expected the answer, got {other:?}"),
        }
        assert!(!bus.is_waiting());
    }
}
