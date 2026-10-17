use serde::de::{self, Deserializer, SeqAccess, Visitor};

pub const G7_TEXT: &str = "secrets is not allowed in [task_templates.<t>] or monorepo.task_defaults; list secrets on each task so every grant is visible on the task and in mise secrets ls";
const G4_TRUE: &str = "secrets = true is not supported; tasks receive only the secrets they list, e.g. secrets = [\"DEPLOY_KEY\"]; grant for one run with mise run --secrets-all <task> or --secrets KEY";
const G4_FALSE: &str =
    "secrets = false is not needed: tasks receive no secrets unless they list them";
const G5_TEXT: &str = "per-task source options (secrets = { fnox = ... }) are not supported yet; list key names: secrets = [\"DEPLOY_KEY\"]";

/// String or list of names. Shape errors fail at parse (G4/G5), like any wrong-typed task
/// field. Name validity (G3/G6) is checked in preflight so a typo cannot break hook-env.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct TaskSecrets(pub Vec<String>);

impl TaskSecrets {
    pub fn names(&self) -> &[String] {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for TaskSecrets {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = TaskSecrets;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("secrets must be a key name or a list of key names")
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
                Ok(TaskSecrets(vec![s.to_string()]))
            }
            fn visit_bool<E: de::Error>(self, b: bool) -> Result<Self::Value, E> {
                Err(E::custom(if b { G4_TRUE } else { G4_FALSE }))
            }
            fn visit_map<A: de::MapAccess<'de>>(self, _: A) -> Result<Self::Value, A::Error> {
                Err(de::Error::custom(G5_TEXT))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut out = vec![];
                loop {
                    match seq.next_element::<toml::Value>() {
                        Ok(Some(toml::Value::String(s))) => out.push(s),
                        Ok(Some(_)) | Err(_) => {
                            return Err(de::Error::custom("secrets entries must be key names"));
                        }
                        Ok(None) => break,
                    }
                }
                Ok(TaskSecrets(out))
            }
        }
        d.deserialize_any(V)
    }
}
