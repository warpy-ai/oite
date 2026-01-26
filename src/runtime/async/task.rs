use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Instant;

// Task state constants for thread-safe state management
pub const TASK_IDLE: u8 = 0;
pub const TASK_SCHEDULED: u8 = 1;
pub const TASK_RUNNING: u8 = 2;
pub const TASK_COMPLETED: u8 = 3;

pub struct Task {
    id: usize,
    future: Mutex<Pin<Box<dyn Future<Output = ()> + Send>>>,
    state: AtomicU8,
    #[cfg(feature = "work-stealing")]
    injector: Option<Arc<crossbeam_deque::Injector<Arc<Task>>>>,
    #[cfg(feature = "work-stealing")]
    unparkers: Option<Arc<Vec<parking::Unparker>>>,
}

impl Task {
    pub fn new<F>(id: usize, future: F) -> Arc<Self>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Arc::new(Self {
            id,
            future: Mutex::new(Box::pin(future)),
            state: AtomicU8::new(TASK_IDLE),
            #[cfg(feature = "work-stealing")]
            injector: None,
            #[cfg(feature = "work-stealing")]
            unparkers: None,
        })
    }

    /// Create a new task with work-stealing support
    #[cfg(feature = "work-stealing")]
    pub fn new_with_executor<F>(
        id: usize,
        future: F,
        injector: Arc<crossbeam_deque::Injector<Arc<Task>>>,
        unparkers: Arc<Vec<parking::Unparker>>,
    ) -> Arc<Self>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Arc::new(Self {
            id,
            future: Mutex::new(Box::pin(future)),
            state: AtomicU8::new(TASK_SCHEDULED),
            injector: Some(injector),
            unparkers: Some(unparkers),
        })
    }

    pub fn id(&self) -> usize {
        self.id
    }

    /// Get the current task state
    pub fn state(&self) -> u8 {
        self.state.load(Ordering::Acquire)
    }

    /// Set task as scheduled (for single-threaded executor)
    pub fn set_scheduled(&self) {
        self.state.store(TASK_SCHEDULED, Ordering::Release);
    }

    /// Check if task is scheduled
    pub fn is_scheduled(&self) -> bool {
        self.state.load(Ordering::Acquire) == TASK_SCHEDULED
    }

    /// Check if task is completed
    pub fn is_completed(&self) -> bool {
        self.state.load(Ordering::Acquire) == TASK_COMPLETED
    }

    pub fn poll(&self, cx: &mut Context) {
        let mut future = self.future.lock().unwrap();
        match future.as_mut().poll(cx) {
            Poll::Ready(()) => {
                self.state.store(TASK_COMPLETED, Ordering::Release);
            }
            Poll::Pending => {
                // Mark as idle so it can be rescheduled
                self.state.store(TASK_IDLE, Ordering::Release);
            }
        }
    }

    /// Poll the task with thread-safe state transitions (for work-stealing)
    #[cfg(feature = "work-stealing")]
    pub fn poll_threadsafe(self: &Arc<Self>) -> bool {
        // Try to transition from SCHEDULED to RUNNING
        if self
            .state
            .compare_exchange(
                TASK_SCHEDULED,
                TASK_RUNNING,
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_err()
        {
            // Already running or completed
            return false;
        }

        // Create a waker that will reschedule this task
        let task_waker = TaskWaker { task: self.clone() };
        let waker = Waker::from(Arc::new(task_waker));
        let mut cx = Context::from_waker(&waker);

        let mut future = self.future.lock().unwrap();
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(()) => {
                self.state.store(TASK_COMPLETED, Ordering::Release);
                true
            }
            Poll::Pending => {
                // Transition back to IDLE so the waker can reschedule
                self.state.store(TASK_IDLE, Ordering::Release);
                true
            }
        }
    }

    pub fn wake(&self) {
        self.state.store(TASK_SCHEDULED, Ordering::Release);
    }
}

/// Waker implementation for work-stealing executor
#[cfg(feature = "work-stealing")]
struct TaskWaker {
    task: Arc<Task>,
}

#[cfg(feature = "work-stealing")]
impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        // Try to transition from IDLE to SCHEDULED
        if self
            .task
            .state
            .compare_exchange(
                TASK_IDLE,
                TASK_SCHEDULED,
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            // Successfully scheduled, push to injector and wake a worker
            if let Some(ref injector) = self.task.injector {
                injector.push(self.task.clone());

                if let Some(ref unparkers) = self.task.unparkers {
                    if !unparkers.is_empty() {
                        let idx = fastrand::usize(..unparkers.len());
                        unparkers[idx].unpark();
                    }
                }
            }
        }
    }
}

pub struct TimerEntry {
    at: Instant,
    task: Arc<Task>,
}

impl TimerEntry {
    pub fn new(at: Instant, task: Arc<Task>) -> Self {
        Self { at, task }
    }

    pub fn at(&self) -> Instant {
        self.at
    }

    pub fn task(&self) -> &Arc<Task> {
        &self.task
    }
}

// BinaryHeap is a max-heap, so we reverse the ordering to get a min-heap (earliest deadline first)
impl PartialEq for TimerEntry {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at
    }
}

impl Eq for TimerEntry {}

impl PartialOrd for TimerEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimerEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering for min-heap behavior (earliest deadline = highest priority)
        other.at.cmp(&self.at)
    }
}

pub struct Timer {
    heap: std::collections::BinaryHeap<TimerEntry>,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            heap: std::collections::BinaryHeap::new(),
        }
    }

    pub fn schedule(&mut self, at: Instant, task: Arc<Task>) {
        self.heap.push(TimerEntry::new(at, task));
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.heap.peek().map(|e| e.at())
    }

    pub fn poll(&mut self) -> Vec<Arc<Task>> {
        let mut expired = Vec::new();
        let now = Instant::now();

        while let Some(entry) = self.heap.peek() {
            if entry.at() <= now {
                let entry = self.heap.pop().unwrap();
                entry.task().set_scheduled();
                expired.push(entry.task().clone());
            } else {
                break;
            }
        }

        expired
    }
}

/// Simple waker for single-threaded executor
struct SimpleWaker {
    task: Arc<Task>,
}

impl Wake for SimpleWaker {
    fn wake(self: Arc<Self>) {
        self.task.set_scheduled();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.task.set_scheduled();
    }
}

pub struct Executor {
    pub tasks: VecDeque<Arc<Task>>,
    pub timer: Mutex<Timer>,
    pub id_gen: AtomicUsize,
}

impl Executor {
    pub fn new() -> Self {
        Self {
            tasks: VecDeque::new(),
            timer: Mutex::new(Timer::new()),
            id_gen: AtomicUsize::new(0),
        }
    }

    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let id = self.id_gen.fetch_add(1, Ordering::SeqCst);
        let task = Task::new(id, future);
        // Note: This is a mutable borrow issue - in single-threaded mode
        // we need interior mutability or to pass &mut self
        // For now, we'll handle this at the call site
    }

    pub fn spawn_task<F>(&mut self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let id = self.id_gen.fetch_add(1, Ordering::SeqCst);
        let task = Task::new(id, future);
        task.set_scheduled();
        self.tasks.push_back(task);
    }

    pub fn spawn_with_id<F>(&mut self, id: usize, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let task = Task::new(id, future);
        task.set_scheduled();
        self.tasks.push_back(task);
    }

    pub fn schedule(&mut self, task: Arc<Task>) {
        task.set_scheduled();
        self.tasks.push_back(task);
    }

    pub fn schedule_at(&mut self, at: Instant, task: Arc<Task>) {
        self.tasks.push_back(task.clone());
        self.timer.lock().unwrap().schedule(at, task);
    }

    pub fn run(&mut self) {
        while !self.tasks.is_empty() {
            // Process all tasks
            let mut pending = VecDeque::new();

            while let Some(task) = self.tasks.pop_front() {
                if task.is_completed() {
                    continue;
                }

                let waker = Arc::new(SimpleWaker { task: task.clone() });
                let waker = Waker::from(waker);
                let mut cx = Context::from_waker(&waker);

                task.poll(&mut cx);

                if !task.is_completed() {
                    pending.push_back(task);
                }
            }

            self.tasks = pending;

            // Add expired timer tasks
            for task in self.timer.lock().unwrap().poll() {
                self.tasks.push_back(task);
            }

            if self.tasks.is_empty() {
                break;
            }

            // Sleep until next timer deadline
            if let Some(deadline) = self.timer.lock().unwrap().next_deadline() {
                let now = Instant::now();
                if deadline > now {
                    std::thread::sleep(deadline - now);
                }
            }
        }
    }

    pub fn try_tick(&mut self) -> bool {
        if let Some(task) = self.tasks.pop_front() {
            if task.is_completed() {
                return true;
            }

            let waker = Arc::new(SimpleWaker { task: task.clone() });
            let waker = Waker::from(waker);
            let mut cx = Context::from_waker(&waker);

            task.poll(&mut cx);

            if !task.is_completed() {
                self.tasks.push_back(task);
            }
            true
        } else {
            false
        }
    }
}

pub struct JoinSet<T> {
    tasks: Mutex<Vec<Arc<Task>>>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> JoinSet<T> {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn spawn<F>(&self, executor: &Executor, future: F)
    where
        F: Future<Output = T> + Send + 'static,
    {
        let id = executor.id_gen.fetch_add(1, Ordering::SeqCst);
        let wrapper = async move {
            future.await;
        };
        let task = Task::new(id, wrapper);
        task.set_scheduled();
        self.tasks.lock().unwrap().push(task);
    }
}
