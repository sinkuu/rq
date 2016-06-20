use protobuf;
use serde;

use descriptor;
use error;
use value;

pub struct Serializer<'a> {
    descriptors: &'a descriptor::Descriptors,
    descriptor: &'a descriptor::MessageDescriptor,
    output: &'a mut protobuf::CodedOutputStream<'a>,
    parents: Vec<&'a descriptor::MessageDescriptor>,
    field: Option<&'a descriptor::FieldDescriptor>,
}

impl<'a> Serializer<'a> {
    pub fn new(descriptors: &'a descriptor::Descriptors,
               descriptor: &'a descriptor::MessageDescriptor,
               output: &'a mut protobuf::CodedOutputStream<'a>)
               -> Serializer<'a> {
        Serializer {
            descriptors: descriptors,
            descriptor: descriptor,
            output: output,
            parents: Vec::new(),
            field: None,
        }
    }

    pub fn for_named_message(descriptors: &'a descriptor::Descriptors,
                             message_name: &str,
                             output: &'a mut protobuf::CodedOutputStream<'a>)
                             -> error::Result<Serializer<'a>> {
        if let Some(message) = descriptors.message_by_name(message_name) {
            Ok(Serializer::new(descriptors, message, output))
        } else {
            Err(error::Error::UnknownMessage(message_name.to_owned()))
        }
    }
}

macro_rules! write_scalar {
    ($ser:expr, $($t:pat => $writer:ident($value:expr)),*) => {
        if let Some(field) = $ser.field.take() {
            match field.field_type($ser.descriptors) {
                $($t => {
                    try!($ser.output.$writer(field.number() as u32, $value));
                    Ok(())
                },)*
                _ => Err(error::Error::UnexpectedType),
            }
        } else {
            Err(error::Error::UnexpectedType)
        }
    }
}

impl<'a> serde::Serializer for Serializer<'a> {
    type Error = error::Error;

    #[inline]
    fn serialize_bool(&mut self, v: bool) -> Result<(), Self::Error> {
        write_scalar!(self, descriptor::FieldType::Bool => write_bool(v))
    }

    fn serialize_i64(&mut self, v: i64) -> Result<(), Self::Error> {
        write_scalar!(self,
                      descriptor::FieldType::Int32 => write_int32(v as i32),
                      descriptor::FieldType::SInt32 => write_sint32(v as i32),
                      descriptor::FieldType::UInt32 => write_uint32(v as u32),
                      descriptor::FieldType::SFixed32 => write_sfixed32(v as i32),
                      descriptor::FieldType::Fixed32 => write_fixed32(v as u32),
                      descriptor::FieldType::Int64 => write_int64(v as i64),
                      descriptor::FieldType::SInt64 => write_sint64(v as i64),
                      descriptor::FieldType::UInt64 => write_uint64(v as u64),
                      descriptor::FieldType::SFixed64 => write_sfixed64(v as i64),
                      descriptor::FieldType::Fixed64 => write_fixed64(v as u64))
    }

    #[inline]
    fn serialize_u64(&mut self, v: u64) -> Result<(), Self::Error> {
        write_scalar!(self,
                      descriptor::FieldType::Int32 => write_int32(v as i32),
                      descriptor::FieldType::SInt32 => write_sint32(v as i32),
                      descriptor::FieldType::UInt32 => write_uint32(v as u32),
                      descriptor::FieldType::SFixed32 => write_sfixed32(v as i32),
                      descriptor::FieldType::Fixed32 => write_fixed32(v as u32),
                      descriptor::FieldType::Int64 => write_int64(v as i64),
                      descriptor::FieldType::SInt64 => write_sint64(v as i64),
                      descriptor::FieldType::UInt64 => write_uint64(v as u64),
                      descriptor::FieldType::SFixed64 => write_sfixed64(v as i64),
                      descriptor::FieldType::Fixed64 => write_fixed64(v as u64))
    }

    #[inline]
    fn serialize_f64(&mut self, v: f64) -> Result<(), Self::Error> {
        write_scalar!(self,
                      descriptor::FieldType::Float => write_float(v as f32),
                      descriptor::FieldType::Double => write_double(v as f64))
    }

    #[inline]
    fn serialize_bytes(&mut self, value: &[u8]) -> Result<(), Self::Error> {
        write_scalar!(self, descriptor::FieldType::Bytes => write_bytes(value))
    }

    #[inline]
    fn serialize_str(&mut self, value: &str) -> Result<(), Self::Error> {
        write_scalar!(self, descriptor::FieldType::String => write_string(value))
    }

    #[inline]
    fn serialize_unit(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    #[inline]
    fn serialize_none(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    #[inline]
    fn serialize_some<V>(&mut self, value: V) -> Result<(), Self::Error> where V: serde::Serialize {
        value.serialize(self)
    }

    #[inline]
    fn serialize_seq<V>(&mut self, visitor: V) -> Result<(), Self::Error>
        where V: serde::ser::SeqVisitor
    {
        if visitor.len() == Some(0) {
            Ok(())
        } else {
            Ok(())
        }
    }

    #[inline]
    fn serialize_seq_elt<T>(&mut self, value: T) -> Result<(), Self::Error>
        where T: serde::Serialize
    {
        Ok(())
    }

    #[inline]
    fn serialize_map<V>(&mut self, mut visitor: V) -> Result<(), Self::Error>
        where V: serde::ser::MapVisitor
    {
        if Some(0) == visitor.len() {
            Ok(())
        } else {
            while let Some(()) = try!(visitor.visit(self)) {}
            Ok(())
        }
    }

    #[inline]
    fn serialize_map_elt<K, V>(&mut self, key: K, value: V) -> Result<(), Self::Error>
        where K: serde::Serialize,
              V: serde::Serialize
    {
        Ok(())
    }
}
