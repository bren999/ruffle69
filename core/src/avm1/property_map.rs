//! The map of property names to values used by the ActionScript VM.
//! This allows for dynamically choosing case-sensitivty at runtime,
//! because SWFv6 and below is case-insensitive. This also maintains
//! the insertion order of properties, which is necessary for accurate
//! enumeration order.

use crate::string::{utils as string_utils, AvmString};
use fnv::FnvBuildHasher;
use gc_arena::Collect;
use indexmap::{Equivalent, IndexMap};
use std::hash::{Hash, Hasher};

type FnvIndexMap<K, V> = IndexMap<K, V, FnvBuildHasher>;

/// A map from property names to values.
#[derive(Default, Clone, Debug)]
pub struct PropertyMap<'gc, V>(FnvIndexMap<PropertyName<'gc>, V>);

impl<'gc, V> PropertyMap<'gc, V> {
    pub fn new() -> Self {
        Self(FnvIndexMap::default())
    }

    pub fn contains_key<T: AsRef<str>>(&self, key: T, case_sensitive: bool) -> bool {
        if case_sensitive {
            self.0.contains_key(&CaseSensitiveStr(key.as_ref()))
        } else {
            self.0.contains_key(&CaseInsensitiveStr(key.as_ref()))
        }
    }

    pub fn entry<'a>(&'a mut self, key: AvmString<'gc>, case_sensitive: bool) -> Entry<'gc, 'a, V> {
        if case_sensitive {
            match self.0.get_index_of(&CaseSensitiveStr(key)) {
                Some(index) => Entry::Occupied(OccupiedEntry {
                    map: &mut self.0,
                    index,
                }),
                None => Entry::Vacant(VacantEntry {
                    map: &mut self.0,
                    key,
                }),
            }
        } else {
            match self.0.get_index_of(&CaseInsensitiveStr(key)) {
                Some(index) => Entry::Occupied(OccupiedEntry {
                    map: &mut self.0,
                    index,
                }),
                None => Entry::Vacant(VacantEntry {
                    map: &mut self.0,
                    key,
                }),
            }
        }
    }

    /// Gets the value for the specified property.
    pub fn get<T: AsRef<str>>(&self, key: T, case_sensitive: bool) -> Option<&V> {
        if case_sensitive {
            self.0.get(&CaseSensitiveStr(key.as_ref()))
        } else {
            self.0.get(&CaseInsensitiveStr(key.as_ref()))
        }
    }

    /// Gets a mutable reference to the value for the specified property.
    pub fn get_mut<T: AsRef<str>>(&mut self, key: T, case_sensitive: bool) -> Option<&mut V> {
        if case_sensitive {
            self.0.get_mut(&CaseSensitiveStr(key.as_ref()))
        } else {
            self.0.get_mut(&CaseInsensitiveStr(key.as_ref()))
        }
    }

    /// Gets a value by index, based on insertion order.
    pub fn get_index(&self, index: usize) -> Option<&V> {
        self.0.get_index(index).map(|(_, v)| v)
    }

    pub fn insert(&mut self, key: AvmString<'gc>, value: V, case_sensitive: bool) -> Option<V> {
        match self.entry(key, case_sensitive) {
            Entry::Occupied(entry) => Some(entry.insert(value)),
            Entry::Vacant(entry) => {
                entry.insert(value);
                None
            }
        }
    }

    /// Returns the value tuples in Flash's iteration order (most recently added first).
    pub fn iter(&self) -> impl Iterator<Item = (AvmString<'gc>, &V)> {
        self.0.iter().rev().map(|(k, v)| (k.0, v))
    }

    /// Returns the key-value tuples in Flash's iteration order (most recently added first).
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (AvmString<'gc>, &mut V)> {
        self.0.iter_mut().rev().map(|(k, v)| (k.0, v))
    }

    pub fn remove<T: AsRef<str>>(&mut self, key: T, case_sensitive: bool) -> Option<V> {
        // Note that we must use shift_remove to maintain order in case this object is enumerated.
        if case_sensitive {
            self.0.shift_remove(&CaseSensitiveStr(key.as_ref()))
        } else {
            self.0.shift_remove(&CaseInsensitiveStr(key.as_ref()))
        }
    }
}

unsafe impl<'gc, V: Collect> Collect for PropertyMap<'gc, V> {
    fn trace(&self, cc: gc_arena::CollectionContext) {
        for (key, value) in &self.0 {
            key.0.trace(cc);
            value.trace(cc);
        }
    }
}

pub enum Entry<'gc, 'a, V> {
    Occupied(OccupiedEntry<'gc, 'a, V>),
    Vacant(VacantEntry<'gc, 'a, V>),
}

pub struct OccupiedEntry<'gc, 'a, V> {
    map: &'a mut FnvIndexMap<PropertyName<'gc>, V>,
    index: usize,
}

impl<'gc, 'a, V> OccupiedEntry<'gc, 'a, V> {
    pub fn remove_entry(&mut self) -> (AvmString<'gc>, V) {
        let (k, v) = self.map.shift_remove_index(self.index).unwrap();
        (k.0, v)
    }

    pub fn get(&self) -> &V {
        self.map.get_index(self.index).unwrap().1
    }

    pub fn get_mut(&mut self) -> &mut V {
        self.map.get_index_mut(self.index).unwrap().1
    }

    pub fn insert(self, value: V) -> V {
        std::mem::replace(self.map.get_index_mut(self.index).unwrap().1, value)
    }
}

pub struct VacantEntry<'gc, 'a, V> {
    map: &'a mut FnvIndexMap<PropertyName<'gc>, V>,
    key: AvmString<'gc>,
}

impl<'gc, 'a, V> VacantEntry<'gc, 'a, V> {
    pub fn insert(self, value: V) {
        self.map.insert(PropertyName(self.key), value);
    }
}

/// Wraps a str-like type, causing the hash map to use a case insensitive hash and equality.
struct CaseInsensitiveStr<T>(T);

impl<T: AsRef<str>> Hash for CaseInsensitiveStr<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        swf_hash_string_ignore_case(self.0.as_ref(), state);
    }
}

impl<'gc, T: AsRef<str>> Equivalent<PropertyName<'gc>> for CaseInsensitiveStr<T> {
    fn equivalent(&self, key: &PropertyName<'gc>) -> bool {
        string_utils::swf_string_eq_ignore_case(&key.0, self.0.as_ref())
    }
}

/// Wraps an str-like type, causing the property map to use a case insensitive hash lookup,
/// but case sensitive equality.
struct CaseSensitiveStr<T>(T);

impl<T: AsRef<str>> Hash for CaseSensitiveStr<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        swf_hash_string_ignore_case(self.0.as_ref(), state);
    }
}

impl<'gc, T: AsRef<str>> Equivalent<PropertyName<'gc>> for CaseSensitiveStr<T> {
    fn equivalent(&self, key: &PropertyName<'gc>) -> bool {
        key.0 == self.0.as_ref()
    }
}

/// The property keys stored in the property map.
/// This uses a case insensitive hash to ensure that properties can be found in
/// SWFv6, which is case insensitive. The equality check is handled by the `Equivalent`
/// impls above, which allow it to be either case-sensitive or insensitive.
/// Note that the property of if key1 == key2 -> hash(key1) == hash(key2) still holds.
#[derive(Debug, Clone, PartialEq, Eq, Collect)]
#[collect(require_static)]
struct PropertyName<'gc>(AvmString<'gc>);

#[allow(clippy::derive_hash_xor_eq)]
impl<'gc> Hash for PropertyName<'gc> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        swf_hash_string_ignore_case(&self.0, state);
    }
}

fn swf_hash_string_ignore_case<H: Hasher>(s: &str, state: &mut H) {
    s.chars()
        .for_each(|c| string_utils::swf_char_to_lowercase(c).hash(state));
    state.write_u8(0xff);
}
