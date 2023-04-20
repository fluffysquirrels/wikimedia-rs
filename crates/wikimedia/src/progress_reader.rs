use std::{
    io::{self, Read},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub struct ProgressReader<R: Read> {
    bytes_read: Arc<AtomicU64>,
    inner: R,
}

impl<R: Read> ProgressReader<R> {
    pub fn new(inner: R) -> (ProgressReader<R>, Arc<AtomicU64>) {
        let bytes_read = Arc::new(AtomicU64::new(0));
        (
            ProgressReader {
                bytes_read: bytes_read.clone(),
                inner,
            },
            bytes_read
        )
    }

    #[allow(dead_code)] // Not used yet.
    pub fn bytes_read(&self) -> Arc<AtomicU64> {
        self.bytes_read.clone()
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buf)?;
        self.bytes_read.fetch_add(u64::try_from(count).expect("usize to u64"), Ordering::SeqCst);
        Ok(count)
    }
}
