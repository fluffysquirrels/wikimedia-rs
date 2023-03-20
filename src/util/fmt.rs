use std::{
    fmt::{Debug, Display},
    time::Duration,
};

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Bytes(pub u64);

#[derive(Clone, Copy)]
pub struct ByteRate(pub f64);

#[derive(Clone)]
pub struct TransferStats {
    /// Transfered file size in bytes.
    pub len: Bytes,

    /// Duration of the file transfer.
    pub duration: Duration,

    /// Transfer rate of the transfer.
    pub rate: ByteRate,
}

impl Debug for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Bytes({num} = {pretty})", num = self.0, pretty = bytes(self.0))
    }
}

impl Display for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&*bytes(self.0))
    }
}

impl Debug for ByteRate {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "ByteRate({num:.0} = {pretty})", num = self.0, pretty = bytes_per_second(self.0))
    }
}

impl Display for ByteRate {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&*bytes_per_second(self.0))
    }
}

impl ByteRate {
    pub fn new(bytes: Bytes, duration: Duration) -> ByteRate {
        let secs = duration.as_secs_f64();
        let rate = if secs.abs() < f64::EPSILON {
            0.
        } else {
            (bytes.0 as f64) / secs
        };

        ByteRate(rate)
    }
}

impl TransferStats {
    pub fn new(len: Bytes, duration: Duration) -> TransferStats {
        TransferStats {
            len,
            duration,
            rate: ByteRate::new(len, duration),
        }
    }
}

impl Debug for TransferStats {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("TransferStats")
         .field("len", &self.len)
         .field("duration", &format!("{:.2?}", self.duration))
         .field("rate", &self.rate)
         .finish()
    }
}

pub fn bytes(len: u64) -> String {
    human_format::Formatter::new()
        .with_scales(human_format::Scales::Binary())
        .with_decimals(2)
        .with_units("B")
        .format(len as f64)
}

pub fn bytes_per_second(rate: f64) -> String {
    human_format::Formatter::new()
        .with_scales(human_format::Scales::Binary())
        .with_decimals(2)
        .with_units("B/s")
        .format(rate)
}
