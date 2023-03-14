//! Based on the macro in the `once_cell` crate documentation:
//! <https://docs.rs/once_cell/1.17.1/once_cell/index.html#lazily-compiled-regex>

macro_rules! lazy_regex {
    ($re:literal $(,)?) => {{
        static RE: once_cell::sync::OnceCell<regex::Regex> = once_cell::sync::OnceCell::new();
        RE.get_or_init(|| regex::Regex::new($re).expect("regex to compile"))
    }};
}
