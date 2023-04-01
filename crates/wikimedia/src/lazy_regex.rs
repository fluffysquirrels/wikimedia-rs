/// Based on the macro in the `once_cell` crate documentation:
/// <https://docs.rs/once_cell/1.17.1/once_cell/index.html#lazily-compiled-regex>
///
/// You must have dependencies on the crates `once_cell` and `regex` to use this macro.
///
/// Returns a value of type [`regex::Regex`].
#[macro_export]
macro_rules! lazy_regex {
    ( $( $re:expr ),+ ) => {{
        static RE: ::once_cell::sync::OnceCell<regex::Regex> = ::once_cell::sync::OnceCell::new();
        RE.get_or_init(|| ::regex::Regex::new( concat!($( $re ),+) ).expect("regex to compile"))
    }};
}
