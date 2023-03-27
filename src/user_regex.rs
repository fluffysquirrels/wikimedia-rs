use regex::{Regex, RegexBuilder};
use std::str::FromStr;
use valuable::{Valuable, Value, Visit};

/// Represents a regex built from user input.
///
/// Implements FromStr with a restricted set of Regex options to try and avoid DoS from malicious input.
#[derive(Clone, Debug)]
pub struct UserRegex(pub Regex);

const MAX_LEN: usize = 100;
const SIZE_LIMIT: usize = 10_000;
const DFA_SIZE_LIMIT: usize = 10_000;
const NEST_LIMIT: u32 = 10;

impl Valuable for UserRegex {
    fn as_value(&self) -> Value<'_> {
        Value::String(self.0.as_str())
    }

    fn visit(&self, visit: &mut dyn Visit) {
        visit.visit_value(self.as_value());
    }
}

impl FromStr for UserRegex {
    type Err = clap::Error;

    fn from_str(s: &str) -> std::result::Result<UserRegex, clap::Error> {
        if s.len() > MAX_LEN {
            return Err(clap::error::Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!("The regex was too long max_len={MAX_LEN} len={len}",
                        len = s.len())));
        }

        let re = RegexBuilder::new(s)
            .size_limit(SIZE_LIMIT)
            .dfa_size_limit(DFA_SIZE_LIMIT)
            .nest_limit(NEST_LIMIT)
            .build()
            .map_err(|e| clap::error::Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!(
                    "Error parsing regex: {e}\n\n\
                     Possibly the regex was too complex. Try and pass a simpler regex.\n\n\
                     To try and prevent denial of service from malicious input, \
                     the regex is built with restricted options (as configured on \
                     regex::RegexBuilder, documentation: \
                     https://docs.rs/regex/latest/regex/struct.RegexBuilder.html ).\n\n\
                     Specifically size_limit={SIZE_LIMIT} dfa_size_limit={DFA_SIZE_LIMIT} \
                     nest_limit={NEST_LIMIT}")))?;
        Ok(UserRegex(re))
    }
}
