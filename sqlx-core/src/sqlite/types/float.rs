use crate::decode::Decode;
use crate::encode::{Encode, IsNull};
use crate::error::BoxDynError;
use crate::sqlite::type_info::DataType;
use crate::sqlite::{Sqlite, SqliteArgumentValue, SqliteTypeInfo, SqliteValueRef};
use crate::types::Type;

impl Type<Sqlite> for f32 {
    fn type_info() -> SqliteTypeInfo {
        SqliteTypeInfo(DataType::Float)
    }
}

impl<'q> Encode<'q, Sqlite> for f32 {
    fn encode_by_ref(&self, args: &mut Vec<SqliteArgumentValue<'q>>) -> IsNull {
        args.push(SqliteArgumentValue::Double((*self).into()));

        IsNull::No
    }

    fn produces(&self) -> Option<SqliteTypeInfo> {
        <Self as Type<Sqlite>>::type_info().into()
    }
}

impl<'r> Decode<'r, Sqlite> for f32 {
    fn accepts(_ty: &SqliteTypeInfo) -> bool {
        true
    }

    fn decode(value: SqliteValueRef<'r>) -> Result<f32, BoxDynError> {
        Ok(value.double() as f32)
    }
}

impl Type<Sqlite> for f64 {
    fn type_info() -> SqliteTypeInfo {
        SqliteTypeInfo(DataType::Float)
    }
}

impl<'q> Encode<'q, Sqlite> for f64 {
    fn encode_by_ref(&self, args: &mut Vec<SqliteArgumentValue<'q>>) -> IsNull {
        args.push(SqliteArgumentValue::Double(*self));

        IsNull::No
    }

    fn produces(&self) -> Option<SqliteTypeInfo> {
        <Self as Type<Sqlite>>::type_info().into()
    }
}

impl<'r> Decode<'r, Sqlite> for f64 {
    fn accepts(_ty: &SqliteTypeInfo) -> bool {
        true
    }

    fn decode(value: SqliteValueRef<'r>) -> Result<f64, BoxDynError> {
        Ok(value.double())
    }
}
