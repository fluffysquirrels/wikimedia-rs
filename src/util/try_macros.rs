/// Analagous to the `std::try!(Result<T,E>)` macro but for use in a function that returns
/// `Option<Result<T,E>>`, such as `Iterator::next()` methods (hence the name).
///
/// Unwraps a `val: Result<T,E>` to a `T` value or if `val` is `Err(e)` returns that early.
///
/// For use in a function that returns `Result<Option<T2>,E2>`.
macro_rules! try_iter {
    ($expr:expr $(,)?) => {
        match $expr {
            Ok(val) => val,
            Err(err) => {
                return Some(Err(err.into()));
            }
        }
    };
}

/// Analagous to the `std::try!(Result<T,E>)` macro but for use on a `Result<Option<T>,E>` value.
///
/// Unwraps a `val: Result<Option<T>,E>` to a `T` value or returns `val` early if `val` is `Err(e)`
/// or `Ok(None)`.
///
/// For use in a function that returns `Result<Option<T2>,E2>`.
macro_rules! try2 {
    ($expr:expr $(,)?) => {
        match $expr {
            std::result::Result::Ok(std::option::Option::Some(val)) => val,
            std::result::Result::Ok(std::option::Option::None) => {
                return std::result::Result::Ok(std::option::Option::None);
            }
            std::result::Result::Err(err) => {
                return std::result::Result::Err(std::convert::From::from(err));
            }
        }
    };
}
