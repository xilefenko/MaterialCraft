//! [`RawObject`]: one object of the serialised tree, generic (class name and named fields), with typed
//! accessors the model builder uses. Nothing here knows what a class means.

use super::packed::PackedList;
use crate::decode::document::Scalar;

/// A field value as stored.
#[derive(Debug, Clone, PartialEq)]
pub enum RawValue {
    Scalar(Scalar),
    Object(Box<RawObject>),
    /// A pointer field whose object is absent (flag `0xff`).
    Absent,
    List(Vec<RawValue>),
    /// A list of flat objects kept packed (the points of brush strokes, see [`PackedList`]).
    Packed(Box<PackedList>),
}

/// One object: its class and its fields in file order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawObject {
    pub class: String,
    pub fields: Vec<(String, RawValue)>,
}

impl RawObject {
    pub fn new(class: &str) -> RawObject {
        RawObject {
            class: class.to_string(),
            ..RawObject::default()
        }
    }

    pub fn field(&self, name: &str) -> Option<&RawValue> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    pub fn scalar(&self, name: &str) -> Option<&Scalar> {
        match self.field(name)? {
            RawValue::Scalar(s) => Some(s),
            _ => None,
        }
    }

    /// Integer-like values (uids, references, enums, ints, channel masks).
    pub fn u64(&self, name: &str) -> Option<u64> {
        self.scalar(name)?.as_u64()
    }

    pub fn f32(&self, name: &str) -> Option<f32> {
        match self.scalar(name)? {
            Scalar::Float(v) => Some(*v),
            _ => None,
        }
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.scalar(name)? {
            Scalar::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn string(&self, name: &str) -> Option<&str> {
        match self.scalar(name)? {
            Scalar::String(v) => Some(v),
            _ => None,
        }
    }

    pub fn floats(&self, name: &str) -> Option<&[f32]> {
        match self.scalar(name)? {
            Scalar::Floats(v) => Some(v),
            _ => None,
        }
    }

    /// The first three components of a vector field.
    pub fn vec3(&self, name: &str) -> Option<[f32; 3]> {
        match self.floats(name)? {
            [a, b, c, ..] => Some([*a, *b, *c]),
            _ => None,
        }
    }

    pub fn ivec2(&self, name: &str) -> Option<[i32; 2]> {
        match self.scalar(name)? {
            Scalar::IVec2(v) => Some(*v),
            _ => None,
        }
    }

    /// The object's own `uid`.
    pub fn uid(&self) -> Option<u64> {
        self.u64("uid")
    }

    /// A pointer or inline object field; `None` if missing or absent.
    pub fn object(&self, name: &str) -> Option<&RawObject> {
        match self.field(name)? {
            RawValue::Object(o) => Some(o),
            _ => None,
        }
    }

    /// The elements of a list field (empty if missing or not a list).
    pub fn list(&self, name: &str) -> &[RawValue] {
        match self.field(name) {
            Some(RawValue::List(items)) => items,
            _ => &[],
        }
    }

    /// A packed list field, if the field is one.
    pub fn packed(&self, name: &str) -> Option<&PackedList> {
        match self.field(name)? {
            RawValue::Packed(p) => Some(p),
            _ => None,
        }
    }

    /// The present objects of a list field, in order.
    pub fn objects<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a RawObject> + 'a + use<'a> {
        self.list(name).iter().filter_map(|v| match v {
            RawValue::Object(o) => Some(o.as_ref()),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_accessors_check_the_tag() {
        let mut o = RawObject::new("DataSourceUniform");
        o.fields.push(("uid".into(), RawValue::Scalar(Scalar::Uid(7))));
        o.fields.push(("opacity".into(), RawValue::Scalar(Scalar::Float(0.5))));
        o.fields
            .push(("color".into(), RawValue::Scalar(Scalar::Floats(vec![0.1, 0.2, 0.3]))));
        o.fields.push(("next".into(), RawValue::Absent));
        assert_eq!(o.uid(), Some(7));
        assert_eq!(o.f32("opacity"), Some(0.5));
        assert_eq!(o.vec3("color"), Some([0.1, 0.2, 0.3]));
        assert_eq!(o.bool("opacity"), None);
        assert!(o.object("next").is_none());
        assert!(o.list("missing").is_empty());
    }
}
