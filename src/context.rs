//! A context map for storing and retrieving values of arbitrary types.
//!
//! This module provides a `Context` struct that can store values of any type
//! specified by the `AnyValueSpec` trait. It also provides a `ContextKeyRegistry`
//! for creating and managing unique keys for accessing values in the context.
//!
//! `Context` uses an internal vector to store values, allowing for efficient
//! insertion and retrieval based on predefined keys.
//!
//! # Examples
//! ```
//! use rushdown::context::{Context, ContextKeyRegistry, UsizeValue, StringValue,
//! ObjectValue};
//!
//! struct Data {
//!    value: usize,
//! }
//!
//! let mut registry = ContextKeyRegistry::default();
//! let key_usize = registry.create::<UsizeValue>();
//! let key_string = registry.create::<StringValue>();
//! let key_object = registry.create::<ObjectValue>();
//! let mut ctx = Context::new();
//! ctx.initialize(&registry);
//! ctx.insert(key_usize, 42usize);
//! ctx.insert(key_string, String::from("Hello, World!"));
//! let data = Box::new(Data { value: 100 });
//! ctx.insert(key_object, data);
//! assert_eq!(ctx.get(key_usize), Some(&42usize));
//! assert_eq!(ctx.get(key_string), Some(&String::from("Hello, World!")));
//! if let Some(retrieved_data) = ctx.get(key_object) {
//!     let downcasted = retrieved_data
//!         .downcast_ref::<Data>()
//!         .expect("Failed to downcast to Data");
//!     assert_eq!(downcasted.value, 100);
//! } else {
//!     panic!("Failed to retrieve object from Context");
//! }
//! ```

extern crate alloc;

use core::any::Any;

use crate::ast::NodeRef;
use crate::util::HashMap;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Specification for values stored in [`Context`].
pub trait AnyValueSpec {
    type Item;

    fn to_any_value(s: Self::Item) -> AnyValue;

    fn from_any_value(v: &AnyValue) -> &Self::Item;

    fn from_any_value_mut(v: &mut AnyValue) -> &mut Self::Item;

    fn pop_any_value(v: AnyValue) -> Self::Item;
}

/// A key for accessing values in an [`Context`].
#[derive(Debug)]
pub struct ContextKey<T: AnyValueSpec> {
    key: usize,
    _marker: core::marker::PhantomData<T>,
}

impl<T: AnyValueSpec> Copy for ContextKey<T> {}

impl<T: AnyValueSpec> Clone for ContextKey<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: AnyValueSpec> ContextKey<T> {
    /// Creates a new [`ContextKey`].
    fn new(key: usize) -> Self {
        Self {
            key,
            _marker: core::marker::PhantomData,
        }
    }

    /// Gets the internal key index.
    #[inline(always)]
    fn key(&self) -> usize {
        self.key
    }
}

impl<T: AnyValueSpec> PartialEq for ContextKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T: AnyValueSpec> Eq for ContextKey<T> {}

/// A registry for creating and managing [`ContextKey`]s.
#[derive(Default, Debug)]
pub struct ContextKeyRegistry {
    current: usize,
    named: HashMap<String, usize>,
}

impl ContextKeyRegistry {
    pub fn new() -> Self {
        Self {
            current: 0,
            named: HashMap::new(),
        }
    }

    /// Creates a new unique [`ContextKey`].
    pub fn create<T: AnyValueSpec>(&mut self) -> ContextKey<T> {
        let v = self.current;
        self.current += 1;
        ContextKey::new(v)
    }

    /// Gets a [`ContextKey`] by name, or creates a new one if it does not exist.
    /// This is useful for sharing keys between different objects.
    pub fn get_or_create<T: AnyValueSpec>(&mut self, name: impl AsRef<str>) -> ContextKey<T> {
        let name = name.as_ref();
        if let Some(key) = self.named.get(name) {
            ContextKey::new(*key)
        } else {
            let key = self.create();
            self.named.insert(String::from(name), key.key());
            key
        }
    }

    pub fn size(&self) -> usize {
        self.current
    }
}

/// A map that can store values of any type specified by [`AnyValueSpec`].
pub struct Context {
    values: Vec<Option<AnyValue>>,
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl Context {
    /// Creates a new empty [`Context`].
    pub fn new() -> Self {
        Self { values: Vec::new() }
    }

    /// Initializes the context with a given [`ContextKeyRegistry`].
    pub fn initialize(&mut self, reg: &ContextKeyRegistry) {
        self.values = (0..reg.size()).map(|_| None).collect();
    }

    /// Inserts a value into the context.
    pub fn insert<T: AnyValueSpec>(&mut self, key: ContextKey<T>, value: T::Item) {
        self.values[key.key()] = Some(T::to_any_value(value));
    }

    /// Gets a reference to a value from the context.
    pub fn get<T: AnyValueSpec>(&self, key: ContextKey<T>) -> Option<&T::Item> {
        if self.values.len() <= key.key() {
            return None;
        }
        if let Some(ref v) = self.values[key.key()] {
            Some(T::from_any_value(v))
        } else {
            None
        }
    }

    /// Gets or inserts a value in the context.
    /// If the key does not exist, inserts the default value and returns a reference to it.
    /// Otherwise, returns a reference to the existing value.
    pub fn get_or_insert<T: AnyValueSpec>(
        &mut self,
        key: ContextKey<T>,
        default: impl FnOnce() -> T::Item,
    ) -> &T::Item {
        if self.values.len() <= key.key() {
            panic!(
                "ContextKey index {} is out of bounds (current size: {})",
                key.key(),
                self.values.len()
            );
        }
        if self.values[key.key()].is_none() {
            self.values[key.key()] = Some(T::to_any_value(default()));
        }
        T::from_any_value(self.values[key.key()].as_ref().unwrap())
    }

    /// Gets a mutable reference to a value from the context.
    pub fn get_mut<T: AnyValueSpec>(&mut self, key: ContextKey<T>) -> Option<&mut T::Item> {
        if self.values.len() <= key.key() {
            return None;
        }
        if let Some(ref mut v) = self.values[key.key()] {
            Some(T::from_any_value_mut(v))
        } else {
            None
        }
    }

    /// Gets or inserts a mutable reference to a value in the context.
    /// If the key does not exist, inserts the default value and returns a mutable reference to it.
    /// Otherwise, returns a mutable reference to the existing value.
    pub fn get_or_insert_mut<T: AnyValueSpec>(
        &mut self,
        key: ContextKey<T>,
        default: impl FnOnce() -> T::Item,
    ) -> &mut T::Item {
        if self.values.len() <= key.key() {
            panic!(
                "ContextKey index {} is out of bounds (current size: {})",
                key.key(),
                self.values.len()
            );
        }
        if self.values[key.key()].is_none() {
            self.values[key.key()] = Some(T::to_any_value(default()));
        }
        T::from_any_value_mut(self.values[key.key()].as_mut().unwrap())
    }

    /// Removes a value from the context.
    /// If the key does not exist, returns `None`.
    /// Otherwise, returns the removed value.
    pub fn remove<T: AnyValueSpec>(&mut self, key: ContextKey<T>) -> Option<T::Item> {
        if self.values.len() <= key.key() {
            return None;
        }
        self.values[key.key()].take().map(T::pop_any_value)
    }
}

/// An enum that can hold any value specified by [`AnyValueSpec`].
#[derive(Debug)]
#[non_exhaustive]
pub enum AnyValue {
    NodeRef(NodeRef),
    Byte(u8),
    Usize(usize),
    Integer(i64),
    Number(f64),
    Bool(bool),
    String(String),
    Object(Box<dyn Any>),
}

/// Specification for `NodeRef` values.
#[derive(Debug)]
pub struct NodeRefValue;

impl AnyValueSpec for NodeRefValue {
    type Item = NodeRef;

    fn to_any_value(s: Self::Item) -> AnyValue {
        AnyValue::NodeRef(s)
    }

    fn from_any_value(v: &AnyValue) -> &Self::Item {
        if let AnyValue::NodeRef(ref s) = v {
            s
        } else {
            panic!("Expected AnyValue::NodeRef, found {:?}", v);
        }
    }

    fn from_any_value_mut(v: &mut AnyValue) -> &mut Self::Item {
        if let AnyValue::NodeRef(ref mut s) = v {
            s
        } else {
            panic!("Expected AnyValue::NodeRef, found {:?}", v);
        }
    }

    fn pop_any_value(v: AnyValue) -> Self::Item {
        if let AnyValue::NodeRef(s) = v {
            s
        } else {
            panic!("Expected AnyValue::NodeRef, found {:?}", v);
        }
    }
}

/// Specification for `u8` values.
#[derive(Debug)]
pub struct ByteValue;

impl AnyValueSpec for ByteValue {
    type Item = u8;

    fn to_any_value(s: Self::Item) -> AnyValue {
        AnyValue::Byte(s)
    }

    fn from_any_value(v: &AnyValue) -> &Self::Item {
        if let AnyValue::Byte(ref s) = v {
            s
        } else {
            panic!("Expected AnyValue::Byte, found {:?}", v);
        }
    }

    fn from_any_value_mut(v: &mut AnyValue) -> &mut Self::Item {
        if let AnyValue::Byte(ref mut s) = v {
            s
        } else {
            panic!("Expected AnyValue::Byte, found {:?}", v);
        }
    }

    fn pop_any_value(v: AnyValue) -> Self::Item {
        if let AnyValue::Byte(s) = v {
            s
        } else {
            panic!("Expected AnyValue::Byte, found {:?}", v);
        }
    }
}

/// Specification for `usize` values.
#[derive(Debug)]
pub struct UsizeValue;

impl AnyValueSpec for UsizeValue {
    type Item = usize;

    fn to_any_value(s: Self::Item) -> AnyValue {
        AnyValue::Usize(s)
    }

    fn from_any_value(v: &AnyValue) -> &Self::Item {
        if let AnyValue::Usize(ref s) = v {
            s
        } else {
            panic!("Expected AnyValue::Usize, found {:?}", v);
        }
    }

    fn from_any_value_mut(v: &mut AnyValue) -> &mut Self::Item {
        if let AnyValue::Usize(ref mut s) = v {
            s
        } else {
            panic!("Expected AnyValue::Usize, found {:?}", v);
        }
    }

    fn pop_any_value(v: AnyValue) -> Self::Item {
        if let AnyValue::Usize(s) = v {
            s
        } else {
            panic!("Expected AnyValue::Usize, found {:?}", v);
        }
    }
}

/// Specification for `i64` values.
#[derive(Debug)]
pub struct IntegerValue;

impl AnyValueSpec for IntegerValue {
    type Item = i64;

    fn to_any_value(s: Self::Item) -> AnyValue {
        AnyValue::Integer(s)
    }

    fn from_any_value(v: &AnyValue) -> &Self::Item {
        if let AnyValue::Integer(ref s) = v {
            s
        } else {
            panic!("Expected AnyValue::Integer, found {:?}", v);
        }
    }

    fn from_any_value_mut(v: &mut AnyValue) -> &mut Self::Item {
        if let AnyValue::Integer(ref mut s) = v {
            s
        } else {
            panic!("Expected AnyValue::Integer, found {:?}", v);
        }
    }

    fn pop_any_value(v: AnyValue) -> Self::Item {
        if let AnyValue::Integer(s) = v {
            s
        } else {
            panic!("Expected AnyValue::Integer, found {:?}", v);
        }
    }
}

/// Specification for `i64` values.
#[derive(Debug)]
pub struct NumberValue;

impl AnyValueSpec for NumberValue {
    type Item = f64;

    fn to_any_value(s: Self::Item) -> AnyValue {
        AnyValue::Number(s)
    }

    fn from_any_value(v: &AnyValue) -> &Self::Item {
        if let AnyValue::Number(ref s) = v {
            s
        } else {
            panic!("Expected AnyValue::Number, found {:?}", v);
        }
    }

    fn from_any_value_mut(v: &mut AnyValue) -> &mut Self::Item {
        if let AnyValue::Number(ref mut s) = v {
            s
        } else {
            panic!("Expected AnyValue::Number, found {:?}", v);
        }
    }

    fn pop_any_value(v: AnyValue) -> Self::Item {
        if let AnyValue::Number(s) = v {
            s
        } else {
            panic!("Expected AnyValue::Number, found {:?}", v);
        }
    }
}

/// Specification for `bool` values.
#[derive(Debug)]
pub struct BoolValue;

impl AnyValueSpec for BoolValue {
    type Item = bool;

    fn to_any_value(s: Self::Item) -> AnyValue {
        AnyValue::Bool(s)
    }

    fn from_any_value(v: &AnyValue) -> &Self::Item {
        if let AnyValue::Bool(ref s) = v {
            s
        } else {
            panic!("Expected AnyValue::Bool, found {:?}", v);
        }
    }

    fn from_any_value_mut(v: &mut AnyValue) -> &mut Self::Item {
        if let AnyValue::Bool(ref mut s) = v {
            s
        } else {
            panic!("Expected AnyValue::Bool, found {:?}", v);
        }
    }

    fn pop_any_value(v: AnyValue) -> Self::Item {
        if let AnyValue::Bool(s) = v {
            s
        } else {
            panic!("Expected AnyValue::Bool, found {:?}", v);
        }
    }
}

/// Specification for `String` values.
#[derive(Debug)]
pub struct StringValue;

impl AnyValueSpec for StringValue {
    type Item = String;

    fn to_any_value(s: Self::Item) -> AnyValue {
        AnyValue::String(s)
    }

    fn from_any_value(v: &AnyValue) -> &Self::Item {
        if let AnyValue::String(ref s) = v {
            s
        } else {
            panic!("Expected AnyValue::String, found {:?}", v);
        }
    }

    fn from_any_value_mut(v: &mut AnyValue) -> &mut Self::Item {
        if let AnyValue::String(ref mut s) = v {
            s
        } else {
            panic!("Expected AnyValue::String, found {:?}", v);
        }
    }

    fn pop_any_value(v: AnyValue) -> Self::Item {
        if let AnyValue::String(s) = v {
            s
        } else {
            panic!("Expected AnyValue::String, found {:?}", v);
        }
    }
}

/// Specification for any values.
#[derive(Debug)]
pub struct ObjectValue;

impl AnyValueSpec for ObjectValue {
    type Item = Box<dyn Any>;

    fn to_any_value(s: Self::Item) -> AnyValue {
        AnyValue::Object(s)
    }

    fn from_any_value(v: &AnyValue) -> &Self::Item {
        if let AnyValue::Object(ref s) = v {
            s
        } else {
            panic!("Expected AnyValue::Object, found {:?}", v);
        }
    }

    fn from_any_value_mut(v: &mut AnyValue) -> &mut Self::Item {
        if let AnyValue::Object(ref mut s) = v {
            s
        } else {
            panic!("Expected AnyValue::Object, found {:?}", v);
        }
    }

    fn pop_any_value(v: AnyValue) -> Self::Item {
        if let AnyValue::Object(s) = v {
            s
        } else {
            panic!("Expected AnyValue::Object, found {:?}", v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(unused_imports)]
    #[cfg(all(not(feature = "std"), feature = "no-std-unix-debug"))]
    use crate::println;

    struct Data(usize);

    #[test]
    fn test_context() {
        let mut registry = ContextKeyRegistry::default();
        let key_node_ref = registry.create::<NodeRefValue>();
        let key_usize = registry.create::<UsizeValue>();
        let key_string = registry.create::<StringValue>();
        let key_bool = registry.create::<BoolValue>();
        let key_object = registry.create::<ObjectValue>();

        let mut ctx = Context::new();
        ctx.initialize(&registry);

        let node_ref = NodeRef::new(1, 1);
        ctx.insert(key_node_ref, node_ref);
        ctx.insert(key_usize, 42usize);
        ctx.insert(key_string, String::from("Hello, World!"));
        ctx.insert(key_bool, true);
        let data = Box::new(Data(100));
        ctx.insert(key_object, data);

        assert_eq!(ctx.get(key_node_ref), Some(&node_ref));
        assert_eq!(ctx.get(key_usize), Some(&42usize));
        assert_eq!(ctx.get(key_string), Some(&String::from("Hello, World!")));
        assert_eq!(ctx.get(key_bool), Some(&true));
        if let Some(retrieved_data) = ctx.get(key_object) {
            let downcasted = retrieved_data
                .downcast_ref::<Data>()
                .expect("Failed to downcast to Data");
            assert_eq!(downcasted.0, 100);
        } else {
            panic!("Failed to retrieve object from Context");
        }

        if let Some(retrieved_data) = ctx.get_mut(key_object) {
            let downcasted = retrieved_data
                .downcast_mut::<Data>()
                .expect("Failed to downcast to Data");
            downcasted.0 += 50;
        } else {
            panic!("Failed to retrieve object from Context");
        }

        if let Some(retrieved_data) = ctx.get(key_object) {
            let downcasted = retrieved_data
                .downcast_ref::<Data>()
                .expect("Failed to downcast to Data");
            assert_eq!(downcasted.0, 150);
        } else {
            panic!("Failed to retrieve object from Context");
        }
    }
}
