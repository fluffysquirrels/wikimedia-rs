/// Lazily constructs a cached instance of [`regex::Regex`][docs_Regex].
///
/// On first invocation the string inside the macro is complied to a
/// `Regex` with [`Regex::new(re: &str)`][docs_Regex_new], then
/// returns the same instance on subsequent invocations of the same
/// macro (e.g. if invoked in a loop body or if used in a function
/// that is called multiple times).
///
/// Returns a value of type `&'static` [`Regex`][docs_Regex].
///
/// The 1 or more arguments given to the macro are concatenated with
/// the [`slice::concat`][slice::concat] method before calling
/// [`Regex::new(re: &str)`][docs_Regex_new] with the result. All
/// arguments must implement the trait `std::borrow::Borrow<str>`,
/// which includes `&str` and `String`. The reason for this feature is
/// to enable re-use of substrings in different regexes. See the unit
/// tests in this source file for an example.
///
/// You must have dependencies on the crates [`once_cell`][docs_once_cell]
/// and [`regex`][docs_regex] to use this macro.
///
/// Based on this macro in the [`once_cell`][docs_once_cell] crate documentation:
/// <https://docs.rs/once_cell/1.17.1/once_cell/index.html#lazily-compiled-regex>
///
/// [docs_Regex]: https://docs.rs/regex/1.7.3/regex/struct.Regex.html
/// [docs_Regex_new]: https://docs.rs/regex/1.7.3/regex/struct.Regex.html#method.new
/// [docs_regex]: https://docs.rs/regex
/// [docs_once_cell]: https://docs.rs/once_cell
/// [slice::concat]: https://doc.rust-lang.org/std/primitive.slice.html#method.concat
#[macro_export]
macro_rules! lazy_regex {
    ( $( $re:expr ),+ ) => {{
        static RE: ::once_cell::sync::OnceCell<regex::Regex> = ::once_cell::sync::OnceCell::new();
        RE.get_or_init(|| {
            let s: String = [ $( $re ),+ ].concat();
            ::regex::Regex::new(&*s).expect("regex to compile")
        })
    }};
}

#[cfg(test)]
mod tests {
    use regex::Regex;

    #[test]
    fn assert_equality_on_multiple_calls() {
        let regexes = (0..=1).map(|_| lazy_regex!("foo"))
                             .collect::<Vec<&'static Regex>>();
        let re1: &'static Regex = regexes[0];
        let re2: &'static Regex = regexes[1];
        assert!(std::ptr::eq(re1, re2));
    }

    #[test]
    fn concat_examples() {
        assert!(lazy_regex!("a").is_match("a"));
        assert!(lazy_regex!("a", "b").is_match("ab"));

        const X: &'static str = "x";

        assert!(lazy_regex!("a", X, "b").is_match("axb"));

        let local_string: String = String::from("foo");
        let local_str: &str = local_string.as_str();

        assert!(lazy_regex!(local_str).is_match("foo"));
    }
}
