use std::collections::BTreeMap;
use std::fmt::Formatter;
use std::str::FromStr;

use either::Either;
use serde::{de, Deserialize};

use crate::task::{EitherIntOrBool, EitherStringOrIntOrBool};

pub struct TomlParser<'a> {
    table: &'a toml::Value,
}

impl<'a> TomlParser<'a> {
    pub fn new(table: &'a toml::Value) -> Self {
        Self { table }
    }

    pub fn parse_str<T>(&self, key: &str) -> Option<T>
    where
        T: From<String>,
    {
        self.table
            .get(key)
            .and_then(|value| value.as_str())
            .map(|value| value.to_string().into())
    }
    pub fn parse_bool(&self, key: &str) -> Option<bool> {
        self.table.get(key).and_then(|value| value.as_bool())
    }
    pub fn parse_array<T>(&self, key: &str) -> Option<Vec<T>>
    where
        T: From<String>,
    {
        self.table
            .get(key)
            .and_then(|value| value.as_array())
            .map(|array| {
                array
                    .iter()
                    .filter_map(|value| value.as_str().map(|v| v.to_string().into()))
                    .collect::<Vec<T>>()
            })
    }
    pub fn parse_table(&self, key: &str) -> Option<BTreeMap<String, toml::Value>> {
        self.table
            .get(key)
            .and_then(|value| value.as_table())
            .map(|table| table.clone().into_iter().collect())
    }
    pub fn parse_env(
        &self,
        key: &str,
    ) -> eyre::Result<Option<BTreeMap<String, EitherStringOrIntOrBool>>> {
        self.table
            .get(key)
            .and_then(|value| value.as_table())
            .map(|table| {
                table
                    .iter()
                    .map(|(key, value)| {
                        let v = value
                            .as_str()
                            .map(|v| Ok(EitherStringOrIntOrBool(Either::Left(v.to_string()))))
                            .or_else(|| {
                                value.as_integer().map(|v| {
                                    Ok(EitherStringOrIntOrBool(Either::Left(v.to_string())))
                                })
                            })
                            .or_else(|| {
                                value.as_bool().map(|v| {
                                    Ok(EitherStringOrIntOrBool(Either::Right(EitherIntOrBool(
                                        Either::Right(v),
                                    ))))
                                })
                            })
                            .unwrap_or_else(|| {
                                Err(eyre::eyre!("invalid env value: {:?}", value))
                            })?;
                        Ok((key.clone(), v))
                    })
                    .collect::<eyre::Result<_>>()
            })
            .transpose()
    }
}

pub fn deserialize_arr<'de, D, T>(deserializer: D) -> eyre::Result<Vec<T>, D::Error>
where
    D: de::Deserializer<'de>,
    T: FromStr + Deserialize<'de>,
    <T as FromStr>::Err: std::fmt::Display,
{
    struct ArrVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T> de::Visitor<'de> for ArrVisitor<T>
    where
        T: FromStr + Deserialize<'de>,
        <T as FromStr>::Err: std::fmt::Display,
    {
        type Value = Vec<T>;
        fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
            formatter.write_str("string or array of strings")
        }

        fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            let v = v.parse().map_err(de::Error::custom)?;
            Ok(vec![v])
        }

        fn visit_seq<S>(self, mut seq: S) -> std::result::Result<Self::Value, S::Error>
        where
            S: de::SeqAccess<'de>,
        {
            let mut v = vec![];
            while let Some(s) = seq.next_element::<String>()? {
                v.push(s.parse().map_err(de::Error::custom)?);
            }
            Ok(v)
        }

        fn visit_map<M>(self, map: M) -> std::result::Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            Ok(vec![Deserialize::deserialize(
                de::value::MapAccessDeserializer::new(map),
            )?])
        }
    }

    deserializer.deserialize_any(ArrVisitor(std::marker::PhantomData))
}
