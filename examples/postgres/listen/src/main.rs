use async_std::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use sqlx::postgres::PgListener;
use sqlx::{Executor, PgPool};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Building PG pool.");
    let conn_str =
        std::env::var("DATABASE_URL").expect("Env var DATABASE_URL is required for this example.");
    let pool = sqlx::PgPool::new(&conn_str).await?;

    let mut listener = PgListener::new(&conn_str).await?;

    // let notify_pool = pool.clone();
    let _t = async_std::task::spawn(async move {
        stream::interval(Duration::from_secs(2))
            .for_each(|_| notify(&pool))
            .await
    });

    println!("Starting LISTEN loop.");

    listener.listen_all(vec!["chan0", "chan1", "chan2"]).await?;

    let mut counter = 0usize;
    loop {
        let notification = listener.recv().await?;
        println!("[from recv]: {:?}", notification);

        counter += 1;
        if counter >= 3 {
            break;
        }
    }

    // Prove that we are buffering messages by waiting for 6 seconds
    listener.execute("SELECT pg_sleep(6)").await?;

    let mut stream = listener.into_stream();
    while let Some(notification) = stream.try_next().await? {
        println!("[from stream]: {:?}", notification);
    }

    Ok(())
}

async fn notify(mut pool: &PgPool) {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    // Note that channel names are lower-cased by Postgres unless they are quoted
    let res = pool
        .execute(&*format!(
            r#"
NOTIFY "chan0", '{{"payload": {}}}';
NOTIFY "chan1", '{{"payload": {}}}';
NOTIFY "chan2", '{{"payload": {}}}';
                "#,
            COUNTER.fetch_add(1, Ordering::SeqCst),
            COUNTER.fetch_add(1, Ordering::SeqCst),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ))
        .await;

    println!("[from notify]: {:?}", res);
}
