use bigdecimal::BigDecimal;

use crate::database::{Database, HasArguments};
use crate::decode::Decode;
use crate::encode::{Encode, IsNull};
use crate::error::BoxDynError;
use crate::mysql::io::MySqlBufMutExt;
use crate::mysql::protocol::text::{ColumnFlags, ColumnType};
use crate::mysql::{MySql, MySqlArguments, MySqlTypeInfo, MySqlValueRef};
use crate::types::Type;

impl Type<MySql> for BigDecimal {
    fn type_info() -> MySqlTypeInfo {
        MySqlTypeInfo::binary(ColumnType::NewDecimal)
    }
}

impl Encode<'_, MySql> for BigDecimal {
    fn encode_by_ref(&self, buf: &mut MySqlArguments) -> IsNull {
        buf.put_str_lenenc(&self.to_string());

        IsNull::No
    }
}

impl Decode<'_, MySql> for BigDecimal {
    fn decode(value: MySqlValueRef<'_>) -> Result<Self, BoxDynError> {
        Ok(value.as_str()?.parse()?)
    }
}
