use crate::decode::Decode;
use crate::encode::{Encode, IsNull};
use crate::error::BoxDynError;
use crate::mysql::{MySql, MySqlTypeInfo, MySqlValueRef};
use crate::types::Type;

impl Type<MySql> for bool {
    fn type_info() -> MySqlTypeInfo {
        // MySQL has no actual `BOOLEAN` type, the type is an alias of `TINYINT(1)`
        <i8 as Type<MySql>>::type_info()
    }
}

impl Encode<'_, MySql> for bool {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> IsNull {
        <i8 as Encode<MySql>>::encode(*self as i8, buf)
    }

    fn produces(&self) -> Option<MySqlTypeInfo> {
        <Self as Type<MySql>>::type_info().into()
    }
}

impl Decode<'_, MySql> for bool {
    fn accepts(ty: &MySqlTypeInfo) -> bool {
        <i8 as Decode<MySql>>::accepts(ty)
    }

    fn decode(value: MySqlValueRef<'_>) -> Result<Self, BoxDynError> {
        Ok(<i8 as Decode<MySql>>::decode(value)? != 0)
    }
}
