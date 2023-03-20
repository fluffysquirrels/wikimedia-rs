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
