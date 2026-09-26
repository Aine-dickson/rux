//! Where an `async` export's future runs.
//!
//! Off the UI thread, always: a future that waits on the network or a disk
//! must not stall a frame. By default each call gets a thread of its own,
//! driven by [`block_on`], which needs no executor crate. An app that has a
//! runtime of its own (tokio, for a database driver that needs one) hands it
//! over once with [`set_spawner`], usually from its `#[rux::init]`.

use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

/// A future to run to its end, with nothing to give back: its answer has
/// already been arranged to go where it must.
pub type Task = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

type Spawner = Arc<dyn Fn(Task) + Send + Sync>;

static SPAWNER: Mutex<Option<Spawner>> = Mutex::new(None);

/// Run every `async` export's future with `spawn` from now on, e.g.
/// `rux::set_spawner(move |task| { handle.spawn(task); })` with a tokio
/// runtime's handle.
pub fn set_spawner(spawn: impl Fn(Task) + Send + Sync + 'static) {
    *SPAWNER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(spawn));
}

/// Run `task` where futures run.
pub fn spawn(task: Task) {
    let spawner = SPAWNER.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match spawner {
        Some(s) => s(task),
        None => default_spawn(task),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn default_spawn(task: Task) {
    let spawned = std::thread::Builder::new().name("rux-native".into()).spawn(move || block_on(task));
    if let Err(e) = spawned {
        // The task is gone with the closure; its answer is sent by the
        // `Drop` of whatever it held to send one with.
        eprintln!("rux: could not start a thread for a native call: {e}");
    }
}

#[cfg(target_arch = "wasm32")]
fn default_spawn(task: Task) {
    // No threads in a browser. Dropping the task drops what it would have
    // answered with, which answers with an error saying so.
    drop(task);
    eprintln!("rux: an async native function needs rux::set_spawner on the web");
}

struct ThreadWaker(std::thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Run `f` to its end on this thread, sleeping while it waits.
pub fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}

/// `f`, with a panic while it is polled turned into `Err` with the panic's
/// text, so one failing native call cannot take the thread's other work, or
/// the app, down with it.
pub struct CatchUnwind<F>(Pin<Box<F>>);

impl<F> CatchUnwind<F> {
    pub fn new(f: F) -> Self {
        CatchUnwind(Box::pin(f))
    }
}

impl<F: Future> Future for CatchUnwind<F> {
    type Output = Result<F::Output, String>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = self.0.as_mut();
        match catch_unwind(AssertUnwindSafe(|| inner.poll(cx))) {
            Ok(Poll::Ready(v)) => Poll::Ready(Ok(v)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(p) => Poll::Ready(Err(panic_text(&*p))),
        }
    }
}

/// What a panic said, when it said it as text.
pub fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "a native function panicked".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_on_waits_for_another_thread() {
        let (tx, rx) = std::sync::mpsc::channel::<i32>();
        let slot = Arc::new(Mutex::new(None::<(i32, Waker)>));
        struct Wait(Arc<Mutex<Option<(i32, Waker)>>>, bool);
        impl Future for Wait {
            type Output = i32;
            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<i32> {
                if self.1 {
                    if let Some((v, _)) = self.0.lock().unwrap().take() {
                        return Poll::Ready(v);
                    }
                }
                self.1 = true;
                *self.0.lock().unwrap() = None;
                let s = Arc::clone(&self.0);
                let w = cx.waker().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    *s.lock().unwrap() = Some((7, w.clone()));
                    w.wake();
                });
                Poll::Pending
            }
        }
        let got = block_on(Wait(slot, false));
        tx.send(got).unwrap();
        assert_eq!(rx.recv().unwrap(), 7);
    }

    #[test]
    fn a_panic_in_a_future_is_an_answer() {
        let r = block_on(CatchUnwind::new(async { panic!("boom") }));
        assert_eq!(r, Err::<(), _>("boom".to_string()));
    }
}
