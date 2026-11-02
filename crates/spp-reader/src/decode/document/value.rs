//! [`Scalar`]: a typed reading of one fixed-size or string value from the object serialisation.

use super::tag::ValueTag;
use std::fmt;

/// One decoded value. Vectors and matrices are kept as their float components.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Float(f32),
    /// vec2/vec3/vec4 and 3×3/4×4 matrices (row order as stored).
    Floats(Vec<f32>),
    Int(i32),
    IVec2([i32; 2]),
    Enum(u64),
    Bool(bool),
    UInt(u32),
    /// The object's own id.
    Uid(u64),
    /// Another object's id.
    UidRef(u64),
    String(String),
    ChannelType(u64),
}

impl Scalar {
    /// Decodes a payload as delivered by [`super::Visitor::scalar`]; `None` if the size does not fit the tag.
    pub fn decode(tag: ValueTag, payload: &[u8]) -> Option<Scalar> {
        let u64_le = || payload.try_into().ok().map(u64::from_le_bytes);
        let u32_le = || payload.try_into().ok().map(u32::from_le_bytes);
        Some(match tag {
            ValueTag::Float => Scalar::Float(f32::from_bits(u32_le()?)),
            ValueTag::Vec2 | ValueTag::Vec3 | ValueTag::Vec4 | ValueTag::Mat3 | ValueTag::Mat4 => {
                if !payload.len().is_multiple_of(4) {
                    return None;
                }
                Scalar::Floats(
                    payload
                        .chunks_exact(4)
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect(),
                )
            }
            ValueTag::Int => Scalar::Int(u32_le()? as i32),
            ValueTag::IVec2 => {
                let v = u64_le()?;
                Scalar::IVec2([v as u32 as i32, (v >> 32) as u32 as i32])
            }
            ValueTag::Enum => Scalar::Enum(u64_le()?),
            ValueTag::Bool => Scalar::Bool(*payload.first()? != 0),
            ValueTag::UInt => Scalar::UInt(u32_le()?),
            ValueTag::Uid => Scalar::Uid(u64_le()?),
            ValueTag::UidRef => Scalar::UidRef(u64_le()?),
            ValueTag::ChannelType => Scalar::ChannelType(u64_le()?),
            ValueTag::String => Scalar::String(String::from_utf8_lossy(payload).into_owned()),
            ValueTag::ValueList | ValueTag::Pointer | ValueTag::PointerList | ValueTag::Value => return None,
        })
    }

    /// The value as an id (uid, reference, enum or integer), if it is one.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Scalar::Uid(v) | Scalar::UidRef(v) | Scalar::Enum(v) | Scalar::ChannelType(v) => Some(*v),
            Scalar::UInt(v) => Some(*v as u64),
            Scalar::Int(v) => u64::try_from(*v).ok(),
            _ => None,
        }
    }
}

impl fmt::Display for Scalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scalar::Float(v) => write!(f, "{v}"),
            Scalar::Floats(v) => write!(f, "{v:?}"),
            Scalar::Int(v) => write!(f, "{v}"),
            Scalar::IVec2(v) => write!(f, "{v:?}"),
            Scalar::Enum(v) => write!(f, "enum {v}"),
            Scalar::Bool(v) => write!(f, "{v}"),
            Scalar::UInt(v) => write!(f, "{v}u"),
            Scalar::Uid(v) => write!(f, "uid {v}"),
            Scalar::UidRef(v) => write!(f, "-> {v}"),
            Scalar::String(v) => write!(f, "{v:?}"),
            Scalar::ChannelType(v) => write!(f, "channel {v}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_by_tag() {
        assert_eq!(
            Scalar::decode(ValueTag::IVec2, &[11, 0, 0, 0, 10, 0, 0, 0]),
            Some(Scalar::IVec2([11, 10]))
        );
        assert_eq!(
            Scalar::decode(ValueTag::Uid, &7u64.to_le_bytes()).and_then(|s| s.as_u64()),
            Some(7)
        );
        assert_eq!(Scalar::decode(ValueTag::Float, &[0, 0]), None);
        assert_eq!(
            Scalar::decode(ValueTag::String, b"ab"),
            Some(Scalar::String("ab".into()))
        );
        assert_eq!(Scalar::decode(ValueTag::Bool, &[0xff]), Some(Scalar::Bool(true)));
    }
}
