use parking_lot::{Condvar, Mutex};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

pub const PRIO_VISIBLE: u8 = 0;
pub const PRIO_TILE: u8 = 1;
pub const PRIO_NEAR: u8 = 2;
pub const PRIO_THUMB: u8 = 3;
pub const PRIO_BACKGROUND: u8 = 4;

struct Job {
    prio: u8,
    seq: u64,
    run: Box<dyn FnOnce() + Send>,
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        self.prio == other.prio && self.seq == other.seq
    }
}
impl Eq for Job {}
impl PartialOrd for Job {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Job {
    fn cmp(&self, other: &Self) -> Ordering {
        other.prio.cmp(&self.prio).then(other.seq.cmp(&self.seq))
    }
}

pub struct Scheduler {
    queue: Mutex<BinaryHeap<Job>>,
    ready: Condvar,
    seq: AtomicU64,
}

impl Scheduler {
    pub fn new(threads: usize) -> Arc<Self> {
        let scheduler = Arc::new(Scheduler { queue: Mutex::new(BinaryHeap::new()), ready: Condvar::new(), seq: AtomicU64::new(0) });
        for i in 0..threads {
            let s = scheduler.clone();
            std::thread::Builder::new()
                .name(format!("render-{i}"))
                .spawn(move || loop {
                    let job = {
                        let mut q = s.queue.lock();
                        loop {
                            if let Some(job) = q.pop() {
                                break job;
                            }
                            s.ready.wait(&mut q);
                        }
                    };
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job.run)).is_err() {
                        eprintln!("render job panicked");
                    }
                })
                .expect("spawn render thread");
        }
        scheduler
    }

    pub fn submit(&self, prio: u8, run: impl FnOnce() + Send + 'static) {
        let seq = self.seq.fetch_add(1, AtomicOrdering::Relaxed);
        self.queue.lock().push(Job { prio, seq, run: Box::new(run) });
        self.ready.notify_one();
    }

    pub async fn run<T: Send + 'static>(&self, prio: u8, f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.submit(prio, move || {
            let _ = tx.send(f());
        });
        rx.await.expect("render job dropped its result")
    }
}

pub fn encode_bmp(width: u32, height: u32, stride: usize, bgr: &[u8]) -> Vec<u8> {
    let row = width as usize * 3;
    let padded = (row + 3) & !3;
    let image_size = padded * height as usize;
    let mut out = Vec::with_capacity(54 + image_size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + image_size) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(-(height as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(image_size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    for y in 0..height as usize {
        out.extend_from_slice(&bgr[y * stride..y * stride + row]);
        out.resize(out.len() + padded - row, 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_rows_are_padded_to_four_bytes() {
        let bmp = encode_bmp(3, 2, 9, &[1u8; 18]);
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(bmp.len(), 54 + 12 * 2);
        assert_eq!(u32::from_le_bytes(bmp[2..6].try_into().unwrap()) as usize, bmp.len());
        assert_eq!(&bmp[54 + 9..54 + 12], &[0, 0, 0]);
    }

    #[test]
    fn higher_priority_jobs_run_first() {
        let s = Scheduler { queue: Mutex::new(BinaryHeap::new()), ready: Condvar::new(), seq: AtomicU64::new(0) };
        let order = Arc::new(Mutex::new(Vec::new()));
        for (prio, tag) in [(PRIO_THUMB, 'c'), (PRIO_VISIBLE, 'a'), (PRIO_TILE, 'b'), (PRIO_VISIBLE, 'z')] {
            let o = order.clone();
            s.submit(prio, move || o.lock().push(tag));
        }
        while let Some(job) = s.queue.lock().pop() {
            (job.run)();
        }
        assert_eq!(*order.lock(), vec!['a', 'z', 'b', 'c']);
    }
}
