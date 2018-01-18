#![feature(nll, box_syntax, iterator_step_by, fnbox)]

use std::sync::{Condvar, Arc, Mutex};
use std::sync::mpsc;
use std::thread;
use std::collections::VecDeque;
use std::boxed::FnBox;

pub struct ThreadPool {
    n_threads: usize,
    tasks: Arc<Mutex<VecDeque<Box<FnBox()+Send>>>>,
    senders: Vec<mpsc::Sender<bool>>,
}

impl ThreadPool {
    pub fn new(n_threads: usize) -> Self {
        let mut senders = Vec::new();
        let tasks: Arc<Mutex<VecDeque<Box<FnBox()+Send>>>> = Arc::new(Mutex::new(VecDeque::new()));
        for _ in 0..n_threads {
            let (sx, tx) = mpsc::channel();
            senders.push(sx);
            let tasks = tasks.clone();
            thread::spawn(move || {
                while tx.try_recv().is_err() {
                    let task = {
                        let mut tasks = tasks.lock().unwrap();
                        tasks.pop_back()
                    };
                    if let Some(task) = task { task(); }
                }
            });
        };
        ThreadPool {
            n_threads: n_threads,
            tasks: tasks,
            senders: senders,
        }
    }

    pub fn submit(&mut self, func: Box<FnBox()+Send>) {
        let mut task = self.tasks.lock().unwrap();
        task.push_front(func);
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        loop {
            let finished = {
                let tasks = self.tasks.lock().unwrap();
                tasks.is_empty()
            };
            if finished {
                for tx in &self.senders { tx.send(true).unwrap(); }
                break;
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
        pool.submit(box (move || { std::thread::sleep_ms(100); let mut v = v1.lock().unwrap(); v.push(1); }));
        pool.submit(box (move || { let mut v = v2.lock().unwrap(); v.push(2); }));
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
            pool.submit(box (move || {
                std::thread::sleep_ms((10-i)*40);
                let mut v = tv.lock().unwrap();
                v.push(i as i64);
            }));
        }
        std::thread::sleep_ms(1000);
        let mut pv = v.lock().unwrap().clone();
        pv.sort();
        assert_eq!(pv, (0..10).collect::<Vec<i64>>());
    }
}
