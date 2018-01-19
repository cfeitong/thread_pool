#![feature(nll, box_syntax, iterator_step_by, fnbox)]

use std::sync::{Condvar, Arc, Mutex};
use std::sync::mpsc;
use std::thread;
use std::collections::VecDeque;
use std::boxed::FnBox;
use std::time::Duration;

static MAX_JOBS: usize = 10;

pub struct ThreadPool {
    max_jobs: usize,
    n_busy_thread: Mutex<usize>,
    tasks: Arc<Mutex<VecDeque<Box<FnBox()+Send>>>>,
    tasks_not_full: Arc<Condvar>,
    tasks_not_empty: Arc<Condvar>,
    not_busy: Arc<Condvar>,
    senders: Vec<mpsc::Sender<bool>>,
}

impl ThreadPool {
    pub fn new(n_threads: usize) -> Self {
        let tasks_not_full = Arc::new(Condvar::new());
        let tasks_not_empty = Arc::new(Condvar::new());
        let not_busy = Arc::new(Condvar::new());
        let mut senders = Vec::new();
        let tasks: Arc<Mutex<VecDeque<Box<FnBox()+Send>>>> = Arc::new(Mutex::new(VecDeque::new()));
        for _ in 0..n_threads {
            let tasks_not_full = tasks_not_full.clone();
            let tasks_not_empty = tasks_not_empty.clone();
            let not_busy = not_busy.clone();
            let (sx, tx) = mpsc::channel();
            senders.push(sx);
            let tasks = tasks.clone();
            thread::spawn(move || {
                'outer: loop {
                    not_busy.notify_all();
                    let task = {
                        let tasks = tasks.lock().unwrap();
                        let mut tasks = tasks_not_empty.wait(tasks).unwrap();
                        while tasks.is_empty() {
                            if tx.try_recv().is_err() {
                                tasks = tasks_not_empty.wait(tasks).unwrap();
                            }
                            else {
                                break 'outer;
                            }
                        }
                        let task = tasks.pop_back();
                        tasks_not_full.notify_all();
                        task
                    };
                    if let Some(task) = task { task(); }
                }
            });
        };
        ThreadPool {
            max_jobs: MAX_JOBS,
            n_busy_thread: Mutex::new(0),
            tasks: tasks,
            tasks_not_full: tasks_not_full,
            tasks_not_empty: tasks_not_empty,
            not_busy: not_busy,
            senders: senders,
        }
    }

    pub fn max_jobs(mut self, n_jobs: usize) -> Self {
        self.max_jobs = n_jobs;
        self
    }

    pub fn submit(&mut self, func: Box<FnBox()+Send>) {
        let mut task = self.tasks.lock().unwrap();
        while !task.is_empty() && task.len() == self.max_jobs {
            task = self.tasks_not_full.wait(task).unwrap();
        }
        task.push_front(func);
        let n_busy_thread = { *self.n_busy_thread.lock().unwrap() };
        if n_busy_thread == self.max_jobs {
            let _ = self.not_busy.wait(task).unwrap();
        }
        self.tasks_not_empty.notify_one();
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        loop {
            let tasks = self.tasks.lock().unwrap();
            if tasks.is_empty() {
                for tx in &self.senders { tx.send(true).unwrap(); }
                self.tasks_not_empty.notify_all();
                break;
            }
            else {
                self.tasks_not_empty.notify_all();
                let _ = self.tasks_not_full.wait(tasks).unwrap();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_pool_create_destroy() {
        let _pool = ThreadPool::new(100);
    }

    #[test]
    fn thread_pool_submit() {
        let mut pool = ThreadPool::new(2);
        let v: Arc<Mutex<Vec<i64>>> = Arc::new(Mutex::new(Vec::new()));
        let v1 = v.clone();
        let v2 = v.clone();
        pool.submit(box move || { std::thread::sleep_ms(100); let mut v = v1.lock().unwrap(); v.push(1); });
        pool.submit(box move || { let mut v = v2.lock().unwrap(); v.push(2); });
        std::thread::sleep_ms(200);
        let pv = v.lock().unwrap().clone();
        assert_eq!(pv, vec![2, 1])
    }

    #[test]
    fn thread_pool_schedule() {
        let mut pool = ThreadPool::new(5);
        let v: Arc<Mutex<Vec<i64>>> = Arc::new(Mutex::new(Vec::new()));
        for i in 0..10 {
            let tv = v.clone();
            pool.submit(box move || {
                std::thread::sleep_ms((10-i)*40);
                let mut v = tv.lock().unwrap();
                v.push(i as i64);
            });
        }
        std::thread::sleep(Duration::from_secs(1));
        let mut pv = v.lock().unwrap().clone();
        // pv.sort();
        assert_eq!(pv, (0..10).collect::<Vec<i64>>());
    }
}
