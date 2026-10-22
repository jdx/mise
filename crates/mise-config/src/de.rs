//! Deserializers shared by several `mise.toml` sections.

use std::fmt::Formatter;
use std::str::FromStr;

use serde::{Deserialize, de};

/// One value or a list of them: a string (parsed with `FromStr`), a table, or a list of either.
pub fn deserialize_arr<'de, D, C, T>(deserializer: D) -> std::result::Result<C, D::Error>
where
    D: de::Deserializer<'de>,
    C: FromIterator<T> + Deserialize<'de>,
    T: FromStr + Deserialize<'de>,
    <T as FromStr>::Err: std::fmt::Display,
{
    struct ArrVisitor<C, T>(std::marker::PhantomData<(C, T)>);

    impl<'de, C, T> de::Visitor<'de> for ArrVisitor<C, T>
    where
        C: FromIterator<T> + Deserialize<'de>,
        T: FromStr + Deserialize<'de>,
        <T as FromStr>::Err: std::fmt::Display,
    {
        type Value = C;
        fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
            formatter.write_str("a string, a map, or a list of strings/maps")
        }

        fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            let v = v.parse().map_err(de::Error::custom)?;
            Ok(std::iter::once(v).collect())
        }

        fn visit_map<M>(self, map: M) -> std::result::Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            let item = T::deserialize(de::value::MapAccessDeserializer::new(map))?;
            Ok(std::iter::once(item).collect())
        }

        fn visit_seq<S>(self, seq: S) -> std::result::Result<Self::Value, S::Error>
        where
            S: de::SeqAccess<'de>,
        {
            let mut seq = seq;
            std::iter::from_fn(|| seq.next_element::<StringOrValue<T>>().transpose())
                .map(|element| match element {
                    Ok(StringOrValue::String(s)) => s.parse().map_err(de::Error::custom),
                    Ok(StringOrValue::Value(v)) => Ok(v),
                    Err(e) => Err(e),
                })
                .collect()
        }
    }

    deserializer.deserialize_any(ArrVisitor(std::marker::PhantomData))
}

/// A list item [`deserialize_arr`] reads: a string, parsed later with `FromStr`, or anything else,
/// which `T` reads itself.
///
/// Written by hand rather than `#[serde(untagged)]`, which buffers the item and reports only that
/// it matched neither; this hands a table straight to `T`, so `T`'s own error (an unknown key, a
/// missing one) is what the user sees.
enum StringOrValue<T> {
    String(String),
    Value(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for StringOrValue<T> {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use de::IntoDeserializer;

        struct ItemVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>> de::Visitor<'de> for ItemVisitor<T> {
            type Value = StringOrValue<T>;

            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("a string or a map")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StringOrValue::String(v.to_string()))
            }

            fn visit_map<M: de::MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                T::deserialize(de::value::MapAccessDeserializer::new(map)).map(StringOrValue::Value)
            }

            fn visit_seq<S: de::SeqAccess<'de>>(self, seq: S) -> Result<Self::Value, S::Error> {
                T::deserialize(de::value::SeqAccessDeserializer::new(seq)).map(StringOrValue::Value)
            }

            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                T::deserialize(v.into_deserializer()).map(StringOrValue::Value)
            }

            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                T::deserialize(v.into_deserializer()).map(StringOrValue::Value)
            }

            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                T::deserialize(v.into_deserializer()).map(StringOrValue::Value)
            }

            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                T::deserialize(v.into_deserializer()).map(StringOrValue::Value)
            }
        }

        deserializer.deserialize_any(ItemVisitor(std::marker::PhantomData))
    }
}

/// The keys a derived `Deserialize` struct reads: the field list serde hands to
/// `deserialize_struct`, renames applied and skipped fields left out.
///
/// # Panics
///
/// When `T` does not deserialize through `deserialize_struct`, so is not a derived struct.
pub fn struct_fields<T: de::DeserializeOwned>() -> &'static [&'static str] {
    struct Fields<'a>(&'a mut &'static [&'static str]);

    impl<'de> de::Deserializer<'de> for Fields<'_> {
        type Error = de::value::Error;

        fn deserialize_any<V: de::Visitor<'de>>(self, _: V) -> Result<V::Value, Self::Error> {
            Err(de::Error::custom("not a struct"))
        }

        fn deserialize_struct<V: de::Visitor<'de>>(
            self,
            _: &'static str,
            fields: &'static [&'static str],
            _: V,
        ) -> Result<V::Value, Self::Error> {
            *self.0 = fields;
            Err(de::Error::custom("only the field names were wanted"))
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
            option unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier
            ignored_any
        }
    }

    let mut fields: &'static [&'static str] = &[];
    let _ = T::deserialize(Fields(&mut fields));
    assert!(!fields.is_empty(), "not a derived struct");
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize, Debug, PartialEq, Eq)]
    #[serde(untagged)]
    enum TestItem {
        String(String),
        Object { a: String, b: i64 },
    }

    impl FromStr for TestItem {
        type Err = String;

        fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
            Ok(TestItem::String(s.to_string()))
        }
    }

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestStruct {
        #[serde(default, deserialize_with = "deserialize_arr")]
        arr: Vec<TestItem>,
    }

    #[test]
    fn test_deserialize_arr_string() {
        let toml_str = r#"arr = "hello""#;
        let expected = TestStruct {
            arr: vec![TestItem::String("hello".to_string())],
        };
        let actual: TestStruct = toml::from_str(toml_str).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_deserialize_arr_string_list() {
        let toml_str = r#"arr = ["hello", "world"]"#;
        let expected = TestStruct {
            arr: vec![
                TestItem::String("hello".to_string()),
                TestItem::String("world".to_string()),
            ],
        };
        let actual: TestStruct = toml::from_str(toml_str).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_deserialize_arr_map() {
        let toml_str = r#"arr = { a = "foo", b = 123 }"#;
        let expected = TestStruct {
            arr: vec![TestItem::Object {
                a: "foo".to_string(),
                b: 123,
            }],
        };
        let actual: TestStruct = toml::from_str(toml_str).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_deserialize_arr_map_list() {
        let toml_str = r#"
        arr = [
            { a = "foo", b = 123 },
            { a = "bar", b = 456 },
        ]
        "#;
        let expected = TestStruct {
            arr: vec![
                TestItem::Object {
                    a: "foo".to_string(),
                    b: 123,
                },
                TestItem::Object {
                    a: "bar".to_string(),
                    b: 456,
                },
            ],
        };
        let actual: TestStruct = toml::from_str(toml_str).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_deserialize_arr_mixed_list() {
        let toml_str = r#"
        arr = [
            "hello",
            { a = "foo", b = 123 },
        ]
        "#;
        let expected = TestStruct {
            arr: vec![
                TestItem::String("hello".to_string()),
                TestItem::Object {
                    a: "foo".to_string(),
                    b: 123,
                },
            ],
        };
        let actual: TestStruct = toml::from_str(toml_str).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn struct_fields_lists_serde_names() {
        #[derive(Deserialize)]
        struct Example {
            plain: String,
            #[serde(rename = "renamed")]
            original: String,
            #[serde(skip)]
            skipped: String,
        }
        assert_eq!(struct_fields::<Example>(), ["plain", "renamed"]);
        let example: Example = toml::from_str("plain = \"a\"\nrenamed = \"b\"").unwrap();
        assert_eq!(
            [example.plain, example.original, example.skipped],
            ["a", "b", ""]
        );
    }
}
