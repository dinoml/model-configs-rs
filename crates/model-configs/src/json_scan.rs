use std::collections::BTreeSet;
use std::fmt;

use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};

pub(crate) struct DuplicateScan {
    pub(crate) pointers: Vec<String>,
    pub(crate) truncated: bool,
}

pub(crate) fn duplicate_keys(source: &[u8]) -> Result<DuplicateScan, serde_json::Error> {
    let mut state = DuplicateState {
        scan: DuplicateScan {
            pointers: Vec::new(),
            truncated: false,
        },
        retained_bytes: 0,
    };
    let mut deserializer = serde_json::Deserializer::from_slice(source);
    DuplicateSeed {
        path: Some(String::new()),
        state: &mut state,
    }
    .deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(state.scan)
}

struct DuplicateState {
    scan: DuplicateScan,
    retained_bytes: usize,
}

impl DuplicateState {
    fn record(&mut self, path: Option<&str>) {
        let Some(path) = path else {
            self.scan.truncated = true;
            return;
        };
        let next_bytes = self.retained_bytes.saturating_add(path.len());
        if self.scan.pointers.len() >= crate::MAX_DUPLICATE_KEY_LOCATIONS
            || next_bytes > crate::MAX_DUPLICATE_KEY_LOCATION_BYTES
        {
            self.scan.truncated = true;
            return;
        }
        self.retained_bytes = next_bytes;
        self.scan.pointers.push(path.to_owned());
    }
}

struct DuplicateSeed<'a> {
    path: Option<String>,
    state: &'a mut DuplicateState,
}

impl<'de> DeserializeSeed<'de> for DuplicateSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for DuplicateSeed<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_string<E>(self, _value: String) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut index = 0_u64;
        loop {
            let path = bounded_index(self.path.as_deref(), index);
            if sequence
                .next_element_seed(DuplicateSeed {
                    path,
                    state: &mut *self.state,
                })?
                .is_none()
            {
                break;
            }
            index = index.saturating_add(1);
        }
        Ok(())
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<String>()? {
            let path = bounded_child(self.path.as_deref(), &key);
            if !keys.insert(key) {
                self.state.record(path.as_deref());
            }
            map.next_value_seed(DuplicateSeed {
                path,
                state: &mut *self.state,
            })?;
        }
        Ok(())
    }
}

fn bounded_child(parent: Option<&str>, key: &str) -> Option<String> {
    let parent = parent?;
    if parent.len().saturating_add(key.len()).saturating_add(1) > crate::MAX_DIAGNOSTIC_TEXT_BYTES {
        return None;
    }
    let child = format!("{parent}/{}", escape_pointer(key));
    (child.len() <= crate::MAX_DIAGNOSTIC_TEXT_BYTES).then_some(child)
}

fn bounded_index(parent: Option<&str>, index: u64) -> Option<String> {
    let parent = parent?;
    let child = format!("{parent}/{index}");
    (child.len() <= crate::MAX_DIAGNOSTIC_TEXT_BYTES).then_some(child)
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
