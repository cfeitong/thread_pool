#![feature(nll, box_syntax, iterator_step_by, fnbox)]

use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::boxed::FnBox;

type Task = Box<FnBox() + Send + 'static>;

struct SharedData {
    empty: (Mutex<bool>, Condvar),
    receiver: Mutex<mpsc::Receiver<Task>>,
    n_threads: AtomicUsize,
    n_active: AtomicUsize,
    n_queued: AtomicUsize,
}

pub struct ThreadPool {
    sender: mpsc::Sender<Task>,
    data: Arc<SharedData>,
}

impl ThreadPool {
    pub fn new(n_threads: usize) -> Self {
        let (tx, rx) = mpsc::channel::<Task>();
        let data = Arc::new(SharedData {
            empty: (Mutex::new(false), Condvar::new()),
            receiver: Mutex::new(rx),
            n_threads: AtomicUsize::new(0),
            n_active: AtomicUsize::new(0),
            n_queued: AtomicUsize::new(0),
        });

        for _ in 0..n_threads {
            let data = data.clone();
            thread::spawn(move || {
                data.n_threads.fetch_add(1, Ordering::SeqCst);

                loop {
                    let msg = {
                        let rx = data.receiver
                            .lock()
                            .expect("working thread fails to lock receiver");
                        rx.recv()
                    };


                    let task = match msg {
                        Ok(task) => task,
                        _ => break,
                    };

                    data.n_active.fetch_add(1, Ordering::SeqCst);

                    task();

                    data.n_queued.fetch_sub(1, Ordering::SeqCst);
                    data.n_active.fetch_sub(1, Ordering::SeqCst);

                    if data.n_active.load(Ordering::SeqCst) == 0 {
                        let &(ref _lock, ref cvar) = &data.empty;
                        cvar.notify_all();
                    }
                }
            });
        }
        ThreadPool {
            sender: tx,
            data: data,
        }
    }

    pub fn submit<F>(&self, func: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let task: Task = box func;
        self.data.n_queued.fetch_add(1, Ordering::SeqCst);
        self.sender.send(task).expect("fail to send task");
    }

    pub fn join(&self) {
        let data = &self.data;
        if data.n_active.load(Ordering::SeqCst) == 0 && data.n_queued.load(Ordering::SeqCst) == 0 {
            return;
        }
        let &(ref lock, ref cvar) = &data.empty;
        let mut lock = lock.lock().unwrap();
        while data.n_active.load(Ordering::SeqCst) != 0 || data.n_queued.load(Ordering::SeqCst) != 0
        {
            lock = cvar.wait(lock).unwrap();
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn thread_pool_create_join() {
        let _pool = ThreadPool::new(100);
        _pool.join();
    }

    #[test]
    fn thread_pool_submit() {
        let pool = ThreadPool::new(2);
        let v: Arc<Mutex<Vec<i64>>> = Arc::new(Mutex::new(Vec::new()));
        let v1 = v.clone();
        let v2 = v.clone();
        pool.submit(move || {
            std::thread::sleep(Duration::from_millis(100));
            let mut v = v1.lock().unwrap();
            v.push(1);
        });
        pool.submit(move || {
            let mut v = v2.lock().unwrap();
            v.push(2);
        });
        pool.join();
        let pv = v.lock().unwrap().clone();
        assert_eq!(pv, vec![2, 1])
    }

    #[test]
    fn thread_pool_schedule() {
        let v: Arc<Mutex<Vec<i64>>> = Arc::new(Mutex::new(Vec::new()));
        let pool = ThreadPool::new(5);
        for i in 0..10 {
            let tv = v.clone();
            pool.submit(move || {
                std::thread::sleep(Duration::from_millis((10 - i) * 40));
                let mut v = tv.lock().unwrap();
                v.push(i as i64);
            });
        }
        pool.join();
        let mut pv = v.lock().unwrap().clone();
        pv.sort();
        assert_eq!(pv, (0..10).collect::<Vec<_>>());
    }
}
