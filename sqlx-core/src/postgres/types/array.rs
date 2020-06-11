use bytes::Buf;

use crate::decode::Decode;
use crate::encode::{Encode, IsNull};
use crate::error::BoxDynError;
use crate::postgres::type_info::PgType;
use crate::postgres::{PgArgumentBuffer, PgTypeInfo, PgValueFormat, PgValueRef, Postgres};
use crate::types::Type;

impl<T> Type<Postgres> for [Option<T>]
where
    [T]: Type<Postgres>,
{
    fn type_info() -> PgTypeInfo {
        <[T] as Type<Postgres>>::type_info()
    }
}

impl<T> Type<Postgres> for Vec<Option<T>>
where
    Vec<T>: Type<Postgres>,
{
    fn type_info() -> PgTypeInfo {
        <Vec<T> as Type<Postgres>>::type_info()
    }
}

impl<'q, T> Encode<'q, Postgres> for Vec<T>
where
    for<'a> &'a [T]: Encode<'q, Postgres>,
    T: Encode<'q, Postgres>,
    Self: Type<Postgres>,
{
    #[inline]
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> IsNull {
        self.as_slice().encode_by_ref(buf)
    }

    fn produces(&self) -> Option<PgTypeInfo> {
        <Self as Type<Postgres>>::type_info().into()
    }
}

impl<'q, T> Encode<'q, Postgres> for &'_ [T]
where
    T: Encode<'q, Postgres> + Type<Postgres>,
    Self: Type<Postgres>,
{
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> IsNull {
        buf.extend(&1_i32.to_be_bytes()); // number of dimensions
        buf.extend(&0_i32.to_be_bytes()); // flags

        // element type
        match T::type_info().0 {
            PgType::DeclareWithName(name) => buf.push_type_hole(&name),

            ty => {
                buf.extend(&ty.oid().to_be_bytes());
            }
        }

        buf.extend(&(self.len() as i32).to_be_bytes()); // len
        buf.extend(&1_i32.to_be_bytes()); // lower bound

        for element in self.iter() {
            // allocate space for the length of the encoded element
            let el_len_offset = buf.len();
            buf.extend(&0_i32.to_be_bytes());

            let el_start = buf.len();

            if let IsNull::Yes = element.encode_by_ref(buf) {
                // NULL is encoded as -1 for a length
                buf[el_len_offset..el_start].copy_from_slice(&(-1_i32).to_be_bytes());
            } else {
                let el_end = buf.len();
                let el_len = el_end - el_start;

                // now we can go back and update the length
                buf[el_len_offset..el_start].copy_from_slice(&(el_len as i32).to_be_bytes());
            }
        }

        IsNull::No
    }

    fn produces(&self) -> Option<PgTypeInfo> {
        <Self as Type<Postgres>>::type_info().into()
    }
}

// TODO: Array decoding in PostgreSQL *could* allow 'r (row) lifetime of elements if we can figure
//       out a way for the TEXT encoding to use some shared memory somewhere.

impl<'r, T> Decode<'r, Postgres> for Vec<T>
where
    T: for<'a> Decode<'a, Postgres> + Type<Postgres>,
    Self: Type<Postgres>,
{
    fn accepts(ty: &PgTypeInfo) -> bool {
        *ty == <Self as Type<Postgres>>::type_info()
    }

    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        let element_type_info = T::type_info();
        let format = value.format();

        match format {
            PgValueFormat::Binary => {
                // https://github.com/postgres/postgres/blob/a995b371ae29de2d38c4b7881cf414b1560e9746/src/backend/utils/adt/arrayfuncs.c#L1548

                let mut buf = value.as_bytes()?;

                // number of dimensions in the array
                let ndim = buf.get_i32();

                if ndim == 0 {
                    // zero dimensions is an empty array
                    return Ok(Vec::new());
                }

                if ndim != 1 {
                    return Err(format!("encountered an array of {} dimensions; only one-dimensional arrays are supported", ndim).into());
                }

                // appears to have been used in the past to communicate potential NULLS
                // but reading source code back through our supported postgres versions (9.5+)
                // this is never used for anything
                let _flags = buf.get_i32();

                // the OID of the element
                let _element_type = buf.get_u32();

                // length of the array axis
                let len = buf.get_i32();

                // the lower bound, we only support arrays starting from "1"
                let lower = buf.get_i32();

                if lower != 1 {
                    return Err(format!("encountered an array with a lower bound of {} in the first dimension; only arrays starting at one are supported", lower).into());
                }

                let mut elements = Vec::with_capacity(len as usize);

                for _ in 0..len {
                    let mut element_len = buf.get_i32();

                    let element_val = if element_len == -1 {
                        element_len = 0;
                        None
                    } else {
                        Some(&buf[..(element_len as usize)])
                    };

                    elements.push(T::decode(PgValueRef {
                        value: element_val,
                        row: None,
                        type_info: element_type_info.clone(),
                        format,
                    })?);

                    buf.advance(element_len as usize);
                }

                Ok(elements)
            }

            PgValueFormat::Text => {
                let s = value.as_str()?;

                // https://github.com/postgres/postgres/blob/a995b371ae29de2d38c4b7881cf414b1560e9746/src/backend/utils/adt/arrayfuncs.c#L718

                // trim the wrapping braces
                let s = &s[1..(s.len() - 1)];

                if s.is_empty() {
                    // short-circuit empty arrays up here
                    return Ok(Vec::new());
                }

                // NOTE: Nearly *all* types use ',' as the sequence delimiter. Yes, there is one
                //       that does not. The BOX (not PostGIS) type uses ';' as a delimiter.

                // TODO: When we add support for BOX we need to figure out some way to make the
                //       delimiter selection

                let delimiter = ',';
                let mut done = false;
                let mut in_quotes = false;
                let mut in_escape = false;
                let mut value = String::with_capacity(10);
                let mut chars = s.chars();
                let mut elements = Vec::with_capacity(4);

                while !done {
                    loop {
                        match chars.next() {
                            Some(ch) => match ch {
                                _ if in_escape => {
                                    value.push(ch);
                                    in_escape = false;
                                }

                                '"' => {
                                    in_quotes = !in_quotes;
                                }

                                '\\' => {
                                    in_escape = true;
                                }

                                _ if ch == delimiter && !in_quotes => {
                                    break;
                                }

                                _ => {
                                    value.push(ch);
                                }
                            },

                            None => {
                                done = true;
                                break;
                            }
                        }
                    }

                    let value_opt = if value == "NULL" {
                        None
                    } else {
                        Some(value.as_bytes())
                    };

                    elements.push(T::decode(PgValueRef {
                        value: value_opt,
                        row: None,
                        type_info: element_type_info.clone(),
                        format,
                    })?);

                    value.clear();
                }

                Ok(elements)
            }
        }
    }
}
