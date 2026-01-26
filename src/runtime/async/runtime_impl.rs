use std::collections::VecDeque;
use std::io;
use std::os::unix::io::RawFd;
use std::pin::Pin;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::{Duration, Instant};

use super::reactor::ReactorHandle;
use super::task::{Executor, Task, Timer};
use super::{Interest, Token};

// ============================================================================
// Single-threaded Runtime
// ============================================================================

pub struct Runtime {
    executor: Executor,
    reactor: Mutex<ReactorHandle>,
    parked: Mutex<Vec<Arc<Task>>>,
    io_events: Mutex<Vec<(Token, Interest)>>,
    threads: Vec<thread::JoinHandle<()>>,
}

impl Runtime {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            executor: Executor::new(),
            reactor: Mutex::new(ReactorHandle::new()?),
            parked: Mutex::new(Vec::new()),
            io_events: Mutex::new(Vec::new()),
            threads: Vec::new(),
        })
    }

    pub fn spawn<F>(&mut self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.executor.spawn_task(future);
    }

    pub fn block_on<F>(&mut self, future: F) -> F::Output
    where
        F: std::future::Future,
    {
        let mut pinned = Box::pin(future);
        let waker_data = Arc::new(WakerData {
            woken: AtomicUsize::new(0),
        });
        let waker = Waker::from(waker_data.clone());

        loop {
            let mut cx = Context::from_waker(&waker);

            match pinned.as_mut().poll(&mut cx) {
                Poll::Ready(output) => return output,
                Poll::Pending => {
                    if waker_data.woken.load(Ordering::Relaxed) == 0 {
                        self.tick();
                    }
                }
            }
        }
    }

    pub fn tick(&mut self) {
        let timeout = self
            .executor
            .timer
            .lock()
            .unwrap()
            .next_deadline()
            .map(|d| {
                let now = Instant::now();
                if d > now {
                    d.duration_since(now).as_millis() as i32
                } else {
                    0
                }
            })
            .unwrap_or(-1);

        let events = self
            .reactor
            .lock()
            .unwrap()
            .wait(timeout)
            .unwrap_or_default();
        self.io_events.lock().unwrap().extend(events);

        // Process expired timers - collect first to avoid borrow conflict
        let expired_tasks: Vec<_> = self.executor.timer.lock().unwrap().poll();
        for task in expired_tasks {
            self.executor.schedule(task);
        }

        // Process all tasks
        let mut pending = VecDeque::new();
        while let Some(task) = self.executor.tasks.pop_front() {
            if task.is_completed() {
                continue;
            }

            let waker_data = Arc::new(WakerData::new());
            let waker = Waker::from(waker_data);
            let mut cx = Context::from_waker(&waker);

            task.poll(&mut cx);

            if !task.is_completed() {
                pending.push_back(task);
            }
        }
        self.executor.tasks = pending;

        self.io_events.lock().unwrap().clear();
    }

    pub fn run<F>(&mut self, future: F) -> F::Output
    where
        F: std::future::Future,
    {
        self.block_on(future)
    }
}

// ============================================================================
// Shared utilities
// ============================================================================

struct WakerData {
    woken: AtomicUsize,
}

impl WakerData {
    fn new() -> Self {
        Self {
            woken: AtomicUsize::new(0),
        }
    }
}

impl Wake for WakerData {
    fn wake(self: Arc<Self>) {
        self.woken.store(1, Ordering::Relaxed);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.store(1, Ordering::Relaxed);
    }
}

pub fn spawn<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    Runtime::new().unwrap().spawn(future);
}

pub fn sleep(duration: Duration) -> Sleep {
    Sleep {
        deadline: Instant::now() + duration,
    }
}

pub struct Sleep {
    deadline: Instant,
}

impl std::future::Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        if Instant::now() >= self.deadline {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
