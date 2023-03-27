use std::{
    fmt::{Debug, Display},
    time::Duration as StdDuration,
};
use valuable::{Fields, NamedField, NamedValues, Structable, StructDef, Valuable, Value, Visit};

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Bytes(pub u64);

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

#[derive(Clone, Copy)]
pub struct Duration(pub StdDuration);

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
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
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
        write!(f, "{:.2?}", self.0)
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
