//! Wrapper types that format in a useful way with Debug, Display or
//! Valuable; typically as a human-readable string.
//!
//! These enable formatting and logging types easily, but also help
//! guarantee at compile time that incompatible types (e.g. byte
//! counts and Vec element counts) are not confused by using the
//! [new type idiom].
//!
//! [new type idiom]: https://doc.rust-lang.org/rust-by-example/generics/new_types.html

use anyhow::bail;
use crate::Result;
use num_bigint::BigUint;
use num_traits::Num;
use sha1::{Digest, Sha1};
use std::{
    fmt::{Debug, Display, Write},
    result::Result as StdResult,
    time::Duration as StdDuration,
};
use valuable::{
    Fields, NamedField, NamedValues, Structable, StructDef, Tuplable, TupleDef, Valuable,
    Value, Visit
};

/// Stores a Sha1Hash as a 20 byte array, but formats with `Debug` or `Display` as a lower case hex string.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Sha1Hash(pub [u8; 20]);

/// Stores a number of bytes as a `u64`, formats with `Display` as a human readable string like "12.53 MiB"
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Bytes(pub u64);

/// Stores a byte transfer rate as bytes per second in a `f64`, formats with `Display` as a human readable string like "12.53 MiB/s"
#[derive(Clone, Copy)]
pub struct ByteRate(pub f64);

#[derive(Clone, Debug, Valuable)]
pub struct TransferStats {
    /// Transfered file size in bytes.
    pub len: Bytes,

    /// Duration of the file transfer.
    pub duration: Duration,

    /// Transfer rate of the transfer.
    pub rate: ByteRate,
}

/// Stores a `std::time::Duration`,
/// but formats with `Debug` or `Display` as a human-readable string like "1h 2m 1s 10ms",
/// and translates with `Valuable` as
///     `{ secs: 3721_u64, nanos: 10_000_000_u32, str: "1h 2m 1s 10ms" }`
#[derive(Clone, Copy)]
pub struct Duration(pub StdDuration);

#[allow(dead_code)] // Used in tests.
const MS:     StdDuration = StdDuration::from_millis(1);
#[allow(dead_code)] // Used in tests.
const SECOND: StdDuration = StdDuration::from_secs(1);
const MINUTE: StdDuration = StdDuration::from_secs(60);
const HOUR:   StdDuration = StdDuration::from_secs(60 * 60);

impl Sha1Hash {
    pub fn from_base36_str(s: &str) -> Result<Sha1Hash> {
        let bu = BigUint::from_str_radix(s, 36)?;
        let bits = bu.bits();
        if bits > 160 {
            bail!("Sha1Hash::from_base36_str error: too many bits in result.\n\
                   -   bits={bits}\n\
                   -   s='{s}'");
        }
        let mut bytes: Vec<u8> = bu.to_bytes_le();
        assert!(bytes.len() <= 20);
        bytes.resize(/* new_len: */ 20 , /* value: */ 0_u8);
        bytes.reverse();
        let bytes_array = <[u8; 20]>::try_from(bytes)
                                     .expect("bytes to be correct length by construction");
        Ok(Sha1Hash(bytes_array))
    }

    pub fn calculate_from_bytes(s: &[u8]) -> Sha1Hash {
        let mut sha1_hasher = Sha1::new();
        sha1_hasher.update(s);
        let sha1_bytes: [u8; 20] = sha1_hasher.finalize().into();
        Sha1Hash(sha1_bytes)
    }
}

impl Debug for Sha1Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Sha1Hash({self})")
    }
}

impl Display for Sha1Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&*hex::encode(self.0))
    }
}

/// Translates as a tuple with a hex-encoded byte string inside, like `("abcdef123")`
impl Valuable for Sha1Hash {
    fn as_value(&self) -> Value<'_> {
        // We don't store a String of the hash, so can't return an &str
        // and use `Value<'a>::String(&'a str)`. Encode as a tuple instead.
        Value::Tuplable(self)
    }

    fn visit(&self, visit: &mut dyn Visit) {
        let s = self.to_string();
        let val = Value::String(&*s);
        visit.visit_unnamed_fields(&[val]);
    }
}

impl Tuplable for Sha1Hash {
    fn definition(&self) -> TupleDef {
        TupleDef::new_static(1)
    }
}

impl serde::Serialize for Sha1Hash {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
        where S: serde::Serializer
    {
        let serializable = valuable_serde::Serializable::new(self);
        serializable.serialize(serializer)
    }
}

#[cfg(test)]
mod sha1_hash_tests {
    use super::Sha1Hash;

    #[test]
    fn example() {
/* Example page XML:
"""
  <page>
    <title>AbacuS</title>
    <ns>0</ns>
    <id>46</id>
    <redirect title="Abacus" />
    <revision>
      <id>783822039</id>
      <parentid>46448989</parentid>
      <timestamp>2017-06-04T21:45:26Z</timestamp>
      <contributor>
        <username>Tom.Reding</username>
        <id>9784415</id>
      </contributor>
      <minor />
      <comment>+{{R category shell}} using [[Project:AWB|AWB]]</comment>
      <model>wikitext</model>
      <format>text/x-wiki</format>
      <text bytes="74" xml:space="preserve">#REDIRECT [[Abacus]]
 
{{Redirect category shell|1=
{{R from CamelCase}}
}}</text>
      <sha1>tdzgf1eon4l1v0cjer5nnwg0y1enxye</sha1>
    </revision>
  </page>
"""
*/

        let text =
"#REDIRECT [[Abacus]]

{{Redirect category shell|1=
{{R from CamelCase}}
}}";
        assert_eq!(text.len(), 74);

        let sha1_base36_str = "tdzgf1eon4l1v0cjer5nnwg0y1enxye";
        let sha1_expected = Sha1Hash::from_base36_str(sha1_base36_str).expect("from_base36_str");

        let sha1_calculated = Sha1Hash::calculate_from_bytes(text.as_bytes());

        assert_eq!(sha1_calculated, sha1_expected);
    }
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

impl Bytes {
    const FIELDS: &[NamedField<'static>] = &[
        NamedField::new("int"),
        NamedField::new("str"),
    ];
}

impl Valuable for Bytes {
    fn as_value(&self) -> Value<'_> {
        Value::Structable(self)
    }

    fn visit(&self, visit: &mut dyn Visit) {
        let s = bytes(self.0);
        visit.visit_named_fields(
            &NamedValues::new(
                Self::FIELDS,
                &[Value::U64(self.0),
                  Value::String(&*s)]))
    }
}

impl Structable for Bytes {
    fn definition(&self) -> StructDef<'_> {
        StructDef::new_static("Bytes", Fields::Named(Self::FIELDS))
    }
}

impl serde::Serialize for Bytes {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
        where S: serde::Serializer
    {
        let serializable = valuable_serde::Serializable::new(self);
        serializable.serialize(serializer)
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
    const FIELDS: &[NamedField<'static>] = &[
        NamedField::new("float"),
        NamedField::new("str"),
    ];
}

impl Valuable for ByteRate {
    fn as_value(&self) -> Value<'_> {
        Value::Structable(self)
    }

    fn visit(&self, visit: &mut dyn Visit) {
        let s = bytes_per_second(self.0);
        visit.visit_named_fields(
            &NamedValues::new(
                Self::FIELDS,
                &[Value::F64(self.0),
                  Value::String(&*s)]))
    }
}

impl Structable for ByteRate {
    fn definition(&self) -> StructDef<'_> {
        StructDef::new_static("ByteRate", Fields::Named(Self::FIELDS))
    }
}

impl ByteRate {
    pub fn new(bytes: Bytes, duration: StdDuration) -> ByteRate {
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
    pub fn new(len: Bytes, duration: StdDuration) -> TransferStats {
        TransferStats {
            len,
            duration: Duration(duration),
            rate: ByteRate::new(len, duration),
        }
    }
}

impl Debug for Duration {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let dur: StdDuration = self.0;
        let mut int_secs = dur.as_secs();

        let mut out = String::new();

        if int_secs >= HOUR.as_secs() {
            let hours = int_secs / HOUR.as_secs();
            int_secs = int_secs % HOUR.as_secs();
            write!(out, "{hours}h")?;
        }

        if int_secs >= MINUTE.as_secs() {
            let mins = int_secs / MINUTE.as_secs();
            int_secs = int_secs % MINUTE.as_secs();
            write!(out, " {mins}m")?;
        }

        if int_secs > 0 {
            write!(out, " {int_secs}s")?;
        }

        let ms = dur.subsec_millis();

        if ms > 0 || out.is_empty() {
            write!(out, " {ms}ms")?;
        }

        let out = out.trim_start();

        f.pad(out)?;

        Ok(())
    }
}

impl Display for Duration {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}

impl Duration {
    const FIELDS: &[NamedField<'static>] = &[
        NamedField::new("secs"),
        NamedField::new("nanos"),
        NamedField::new("str"),
    ];
}

impl Valuable for Duration {
    fn as_value(&self) -> Value<'_> {
        Value::Structable(self)
    }

    fn visit(&self, visit: &mut dyn Visit) {
        let s = format!("{:?}", self);
        visit.visit_named_fields(
            &NamedValues::new(
                Self::FIELDS,
                &[Value::U64(self.0.as_secs()),
                  Value::U32(self.0.subsec_nanos()),
                  Value::String(&*s)]))
    }
}

impl Structable for Duration {
    fn definition(&self) -> StructDef<'_> {
        StructDef::new_static("Duration", Fields::Named(Self::FIELDS))
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

pub fn chrono_time<Tz: chrono::TimeZone>(dt: chrono::DateTime<Tz>) -> String
    where <Tz as chrono::TimeZone>::Offset: Display
{
    dt.to_rfc3339_opts(chrono::SecondsFormat::Secs,
                       true /* use_z */)
      .replace('T', " ")
}

#[cfg(test)]
mod tests {
    use super::{Duration, MS, SECOND, MINUTE, HOUR};

    macro_rules! case {
        ($input:expr, $expected:literal) => {
            ($input, $expected, format!("Case from {file}:{linum}:\n\
                                         |    input:    {input}\n\
                                         |    expected: {expected}",
                                        file = file!(),
                                        linum = line!(),
                                        input = stringify!($input),
                                        expected = stringify!($expected)))
        }
    }

    #[test]
    fn duration_formatting() {
        let cases = &[
            case!(SECOND * 3,                                   "3s"           ),
            case!(MS * 333,                                     "333ms"        ),
            case!(SECOND + MS * 333,                            "1s 333ms"     ),
            case!(MINUTE * 2,                                   "2m"           ),
            case!(MINUTE * 2 + SECOND * 1,                      "2m 1s"        ),
            case!(HOUR * 1 + MINUTE * 2 + SECOND * 1,           "1h 2m 1s"     ),
            case!(HOUR * 1 + MINUTE * 2 + SECOND * 1 + MS * 10, "1h 2m 1s 10ms"),
        ];

        let mut fails: u64 = 0;

        for (input, expected, label) in cases.iter() {
            let input = Duration(input.clone());
            let output = input.to_string(); // format!("{input:?}");
            println!("{label}\n\
                      |    output:   \"{output}\"\n");

            if *expected == &*output {
                println!("OK");
            } else {
                println!("FAIL!");
                fails += 1;
            }
            println!("----\n");
        }

        println!("fails = {fails}\n\n");

        assert!(fails == 0);
    }

    #[test]
    fn duration_padding() {
        let dur = Duration(SECOND * 2);
        assert_eq!(&*format!("{dur:>6}"), "    2s");
        assert_eq!(&*format!("{dur:<6}"), "2s    ");
    }
}
