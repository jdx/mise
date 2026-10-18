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
            #[derive(Deserialize)]
            #[serde(untagged)]
            enum StringOrValue<T> {
                String(String),
                Value(T),
            }
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
