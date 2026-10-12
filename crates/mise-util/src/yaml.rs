//! YAML reading and writing on top of `serde-saphyr`, plus [`Value`] for code
//! that reads YAML without a fixed schema.
//!
//! Every parse goes through [`options`] so mise reads YAML the same way
//! everywhere: only `true`/`false` are booleans when the target type does not
//! say otherwise, so `yes`, `on`, `y` and `no` stay strings. A non-finite
//! float (`.inf`, `.nan`, `1e999`) with no declared type is an error rather
//! than a value: serde-saphyr would otherwise hand it over as a string.

use indexmap::IndexMap;
use serde::de::{self, DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::hash::{Hash, Hasher};

pub use serde_saphyr::Error;

/// The parser options every mise YAML read uses.
pub fn options() -> serde_saphyr::Options {
    serde_saphyr::options! {
        strict_booleans: true,
    }
}

pub fn from_str<T: DeserializeOwned>(text: &str) -> Result<T, Error> {
    serde_saphyr::from_str_with_options(text, options())
}

pub fn to_string<T: Serialize>(value: &T) -> Result<String, serde_saphyr::ser::Error> {
    serde_saphyr::to_string(value)
}

/// Mapping keys keep the order they appear in the document.
pub type Mapping = IndexMap<Value, Value>;

/// Any YAML node. Tags are dropped: `!foo bar` reads as the string `bar`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Sequence(Vec<Value>),
    Mapping(Mapping),
}

impl Value {
    /// The value under a string key, when this is a mapping.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_mapping()?.get(&Value::from(key))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_mapping(&self) -> Option<&Mapping> {
        match self {
            Value::Mapping(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_sequence(&self) -> Option<&[Value]> {
        match self {
            Value::Sequence(s) => Some(s),
            _ => None,
        }
    }
}

/// Mappings compare without regard to key order, so they hash only their
/// length.
impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Value::Null => {}
            Value::Bool(b) => b.hash(state),
            Value::Number(n) => n.hash(state),
            Value::String(s) => s.hash(state),
            Value::Sequence(s) => s.hash(state),
            Value::Mapping(m) => m.len().hash(state),
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Number(n.into())
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Number(n.into())
    }
}

/// A YAML number. Integers that fit stay exact; anything else is a float.
#[derive(Clone, Copy, Debug)]
pub enum Number {
    PosInt(u64),
    NegInt(i64),
    Float(f64),
}

impl Number {
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Number::PosInt(n) => i64::try_from(n).ok(),
            Number::NegInt(n) => Some(n),
            Number::Float(_) => None,
        }
    }

    pub fn as_f64(&self) -> f64 {
        match *self {
            Number::PosInt(n) => n as f64,
            Number::NegInt(n) => n as f64,
            Number::Float(n) => n,
        }
    }
}

impl From<i64> for Number {
    fn from(n: i64) -> Self {
        match u64::try_from(n) {
            Ok(n) => Number::PosInt(n),
            Err(_) => Number::NegInt(n),
        }
    }
}

impl From<u64> for Number {
    fn from(n: u64) -> Self {
        Number::PosInt(n)
    }
}

impl From<f64> for Number {
    fn from(n: f64) -> Self {
        Number::Float(n)
    }
}

/// Floats compare by bit pattern so `Number` can be a mapping key: `.nan`
/// equals itself, and `0.0` and `-0.0` are different keys.
impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Number::PosInt(a), Number::PosInt(b)) => a == b,
            (Number::NegInt(a), Number::NegInt(b)) => a == b,
            (Number::Float(a), Number::Float(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}

impl Eq for Number {}

impl Hash for Number {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match *self {
            Number::PosInt(n) => (0u8, n).hash(state),
            Number::NegInt(n) => (1u8, n).hash(state),
            Number::Float(n) => (2u8, n.to_bits()).hash(state),
        }
    }
}

/// Integers print as written; floats print with a fractional part (`1.0`)
/// or in YAML's spelling for the non-finite ones (`.inf`, `-.inf`, `.nan`).
impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Number::PosInt(n) => write!(f, "{n}"),
            Number::NegInt(n) => write!(f, "{n}"),
            Number::Float(n) if n.is_nan() => f.write_str(".nan"),
            Number::Float(n) if n.is_infinite() => {
                f.write_str(if n > 0.0 { ".inf" } else { "-.inf" })
            }
            Number::Float(n) => write!(f, "{n:?}"),
        }
    }
}

impl Serialize for Number {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match *self {
            Number::PosInt(n) => serializer.serialize_u64(n),
            Number::NegInt(n) => serializer.serialize_i64(n),
            Number::Float(n) => serializer.serialize_f64(n),
        }
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Number(n) => n.serialize(serializer),
            Value::String(s) => serializer.serialize_str(s),
            Value::Sequence(s) => s.serialize(serializer),
            Value::Mapping(m) => m.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any YAML value")
    }

    fn visit_bool<E: de::Error>(self, b: bool) -> Result<Value, E> {
        Ok(Value::Bool(b))
    }

    fn visit_i64<E: de::Error>(self, n: i64) -> Result<Value, E> {
        Ok(Value::Number(n.into()))
    }

    fn visit_u64<E: de::Error>(self, n: u64) -> Result<Value, E> {
        Ok(Value::Number(n.into()))
    }

    fn visit_i128<E: de::Error>(self, n: i128) -> Result<Value, E> {
        Ok(match (u64::try_from(n), i64::try_from(n)) {
            (Ok(n), _) => Value::Number(n.into()),
            (_, Ok(n)) => Value::Number(n.into()),
            _ => Value::Number((n as f64).into()),
        })
    }

    fn visit_u128<E: de::Error>(self, n: u128) -> Result<Value, E> {
        Ok(match u64::try_from(n) {
            Ok(n) => Value::Number(n.into()),
            Err(_) => Value::Number((n as f64).into()),
        })
    }

    fn visit_f64<E: de::Error>(self, n: f64) -> Result<Value, E> {
        Ok(Value::Number(n.into()))
    }

    fn visit_str<E: de::Error>(self, s: &str) -> Result<Value, E> {
        Ok(Value::String(s.to_string()))
    }

    fn visit_string<E: de::Error>(self, s: String) -> Result<Value, E> {
        Ok(Value::String(s))
    }

    fn visit_bytes<E: de::Error>(self, b: &[u8]) -> Result<Value, E> {
        Ok(Value::String(String::from_utf8_lossy(b).into_owned()))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        Value::deserialize(deserializer)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Value, D::Error> {
        Value::deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(v) = seq.next_element()? {
            out.push(v);
        }
        Ok(Value::Sequence(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut out = Mapping::with_capacity(map.size_hint().unwrap_or(0));
        while let Some((k, v)) = map.next_entry()? {
            out.insert(k, v);
        }
        Ok(Value::Mapping(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn reads_yaml_1_1_booleans_as_strings() {
        let v: Value = from_str("a: yes\nb: on\nc: no\nd: true\ne: False\n").unwrap();
        assert_eq!(v.get("a"), Some(&Value::from("yes")));
        assert_eq!(v.get("b"), Some(&Value::from("on")));
        assert_eq!(v.get("c"), Some(&Value::from("no")));
        assert_eq!(v.get("d"), Some(&Value::Bool(true)));
        assert_eq!(v.get("e"), Some(&Value::Bool(false)));
    }

    #[test]
    fn keeps_mapping_order_and_non_string_keys() {
        let v: Value = from_str("z: 1\n386: i686\na: [x, 2.5, ~]\n").unwrap();
        let keys: Vec<_> = v.as_mapping().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            vec![Value::from("z"), Value::from(386), Value::from("a")]
        );
        assert_eq!(
            v.get("a"),
            Some(&Value::Sequence(vec![
                Value::from("x"),
                Value::from(2.5),
                Value::Null
            ]))
        );
    }

    #[test]
    fn number_display() {
        assert_eq!(Number::from(1.0).to_string(), "1.0");
        assert_eq!(Number::from(1.5).to_string(), "1.5");
        assert_eq!(Number::from(-3i64).to_string(), "-3");
        assert_eq!(Number::from(u64::MAX).to_string(), u64::MAX.to_string());
        assert_eq!(Number::from(f64::INFINITY).to_string(), ".inf");
        assert_eq!(Number::from(f64::NAN).to_string(), ".nan");
    }

    #[test]
    fn round_trips_ambiguous_strings() {
        let v: Value = from_str("a: 'yes'\nb: '1.0'\nc: 'null'\nd: ''\ne: \"386\"\n").unwrap();
        let text = to_string(&v).unwrap();
        assert_eq!(from_str::<Value>(&text).unwrap(), v);
    }

    #[test]
    fn untyped_non_finite_floats_are_rejected() {
        for text in ["a: .inf\n", "a: -.inf\n", "a: .nan\n", "a: 1e999\n"] {
            assert!(from_str::<Value>(text).is_err(), "{text}");
        }
        // a field that asks for a string still gets the text
        let map: IndexMap<String, String> = from_str("a: .inf\n").unwrap();
        assert_eq!(map["a"], ".inf");
    }

    #[test]
    fn empty_document_is_null() {
        assert_eq!(from_str::<Value>("").unwrap(), Value::Null);
        assert_eq!(
            from_str::<Value>("# only a comment\n").unwrap(),
            Value::Null
        );
    }
}
