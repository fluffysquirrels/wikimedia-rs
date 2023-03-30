use rand::RngCore;
use std::fmt::Write;

pub fn rand_hex(len: usize) -> String {
    let mut rng = rand::thread_rng();

    let mut bytes = Vec::<u8>::with_capacity(len);
    bytes.resize(len, 0_u8);
    rng.fill_bytes(bytes.as_mut());

    let mut out = String::with_capacity(len * 2);

    for byte in bytes.into_iter() {
        write!(out, "{byte:02x}").expect("no failures writing to a String");
    }

    out
}
