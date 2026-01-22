use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::future::Future;
use std::time::Instant;

pub struct Task {
    id: usize,
    future: Mutex<Pin<Box<dyn Future<Output = ()> + Send>>>,
    waker: Option<Waker>,
    scheduled: bool,
}

impl Task {
    pub fn new<F>(id: usize, future: F) -> Arc<Self>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Arc::new(Self {
            id,
            future: Mutex::new(Box::pin(future)),
            waker: None,
            scheduled: false,
        })
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn poll(&self, cx: &mut Context) {
        let mut future = self.future.lock().unwrap();
        match future.as_mut().poll(cx) {
            Poll::Ready(()) => {}
            Poll::Pending => {
                if !self.scheduled {
                    self.scheduled = true;
                }
            }
        }
    }

    pub fn wake(&self) {
        self.scheduled = true;
        if let Some(waker) = &self.waker {
            waker.wake_by_ref();
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
                entry.task().scheduled = true;
                expired.push(entry.task().clone());
            } else {
                break;
            }
        }

        expired
    }
}

pub struct Executor {
    tasks: VecDeque<Arc<Task>>,
    timer: Mutex<Timer>,
    id_gen: std::sync::atomic::AtomicUsize,
}

impl Executor {
    pub fn new() -> Self {
        Self {
            tasks: VecDeque::new(),
            timer: Mutex::new(Timer::new()),
            id_gen: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let id = self.id_gen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let task = Task::new(id, future);
        self.tasks.push_back(task);
    }

    pub fn spawn_with_id<F>(&self, id: usize, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let task = Task::new(id, future);
        self.tasks.push_back(task);
    }

    pub fn schedule(&self, task: Arc<Task>) {
        self.tasks.push_back(task);
    }

    pub fn schedule_at(&self, at: Instant, task: Arc<Task>) {
        self.tasks.push_back(task);
        self.timer.lock().unwrap().schedule(at, task);
    }

    pub fn run(&mut self) {
        while !self.tasks.is_empty() {
            let now = Instant::now();

            self.tasks.retain(|task| {
                task.scheduled = false;
                let waker = task.waker.take().unwrap_or_else(|| {
                    let task = task.clone();
                    Arc::new(move || task.wake()) as Arc<dyn Fn() + Send + Sync>
                });
                let cx = &mut Context::from_waker(&Waker::from(waker));
                task.poll(cx);
                !task.future.lock().unwrap().as_mut().poll(cx).is_ready()
            });

            for task in self.timer.lock().unwrap().poll() {
                self.tasks.push_back(task);
            }

            if self.tasks.is_empty() {
                break;
            }

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
            task.scheduled = false;
            let waker = task.waker.take().unwrap_or_else(|| {
                let task = task.clone();
                Arc::new(move || task.wake()) as Arc<dyn Fn() + Send + Sync>
            });
            let cx = &mut Context::from_waker(&Waker::from(waker));
            task.poll(cx);
            if !task.future.lock().unwrap().as_mut().poll(cx).is_ready() {
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
        let id = executor.id_gen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let wrapper = async move {
            future.await;
        };
        let task = Task::new(id, wrapper);
        self.tasks.lock().unwrap().push(task);
    }
}
