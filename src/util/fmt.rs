pub fn bytes(len: u64) -> String {
    human_format::Formatter::new()
        .with_scales(human_format::Scales::SI())
        .with_decimals(2)
        .with_units("B")
        .format(len as f64)
}
