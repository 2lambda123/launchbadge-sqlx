use sqlx::MySql;
use sqlx_test::new;

#[cfg_attr(feature = "runtime-async-std", async_std::test)]
#[cfg_attr(feature = "runtime-tokio", tokio::test)]
async fn macro_select_from_cte() -> anyhow::Result<()> {
    let mut conn = new::<MySql>().await?;
    let account =
        sqlx::query!("select * from (select (1) as id, 'Herp Derpinson' as name, cast(null as char) email) accounts")
            .fetch_one(&mut conn)
            .await?;

    assert_eq!(account.id, 1);
    assert_eq!(account.name, "Herp Derpinson");
    // MySQL can tell us the nullability of expressions, ain't that cool
    assert_eq!(account.email, None);

    Ok(())
}

#[cfg_attr(feature = "runtime-async-std", async_std::test)]
#[cfg_attr(feature = "runtime-tokio", tokio::test)]
async fn macro_select_from_cte_bind() -> anyhow::Result<()> {
    let mut conn = new::<MySql>().await?;
    let account = sqlx::query!(
        "select * from (select (1) as id, 'Herp Derpinson' as name) accounts where id = ?",
        1i32
    )
    .fetch_one(&mut conn)
    .await?;

    println!("{:?}", account);
    println!("{}: {}", account.id, account.name);

    Ok(())
}

#[derive(Debug)]
struct RawAccount {
    r#type: i32,
    name: Option<String>,
}

#[cfg_attr(feature = "runtime-async-std", async_std::test)]
#[cfg_attr(feature = "runtime-tokio", tokio::test)]
async fn test_query_as_raw() -> anyhow::Result<()> {
    let mut conn = new::<MySql>().await?;

    let account = sqlx::query_as!(
        RawAccount,
        "SELECT * from (select 1 as type, cast(null as char) as name) accounts"
    )
    .fetch_one(&mut conn)
    .await?;

    assert_eq!(account.name, None);
    assert_eq!(account.r#type, 1);

    println!("{:?}", account);

    Ok(())
}

#[cfg_attr(feature = "runtime-async-std", async_std::test)]
#[cfg_attr(feature = "runtime-tokio", tokio::test)]
async fn test_query_bytes() -> anyhow::Result<()> {
    let mut conn = new::<MySql>().await?;

    let rec = sqlx::query!("SELECT X'01AF' as _1")
        .fetch_one(&mut conn)
        .await?;

    assert_eq!(rec._1, &[0x01_u8, 0xAF_u8]);

    Ok(())
}
