//! Store [`ChapterNumber`] as TEXT (`"12.5"`) in any sqlx database.
//!
//! These impls must live here because of the orphan rule: a trait from
//! one crate (sqlx) can only be implemented for a type from another crate
//! by the crate that owns one of them. The keys and enums get theirs from
//! `#[derive(sqlx::Type)]` behind the same feature.

use sqlx::{Database, Decode, Encode, Type, encode::IsNull, error::BoxDynError};

use crate::ChapterNumber;

impl<DB: Database> Type<DB> for ChapterNumber
where
    String: Type<DB>,
{
    fn type_info() -> DB::TypeInfo {
        <String as Type<DB>>::type_info()
    }

    fn compatible(ty: &DB::TypeInfo) -> bool {
        <String as Type<DB>>::compatible(ty)
    }
}

impl<'q, DB: Database> Encode<'q, DB> for ChapterNumber
where
    String: Encode<'q, DB>,
{
    fn encode_by_ref(&self, buf: &mut DB::ArgumentBuffer) -> Result<IsNull, BoxDynError> {
        self.to_string().encode(buf)
    }
}

impl<'r, DB: Database> Decode<'r, DB> for ChapterNumber
where
    String: Decode<'r, DB>,
{
    fn decode(value: DB::ValueRef<'r>) -> Result<Self, BoxDynError> {
        Ok(String::decode(value)?.parse()?)
    }
}
