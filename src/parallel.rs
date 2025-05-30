use crate::Result;
use crate::config::Settings;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub async fn parallel<T, F, Fut, U>(input: Vec<T>, f: F) -> Result<Vec<U>>
where
    T: Send + 'static,
    U: Send + 'static,
    F: Fn(T) -> Fut + Send + Copy + 'static,
    Fut: Future<Output = Result<U>> + Send + 'static,
{
    let semaphore = Arc::new(Semaphore::new(Settings::get().jobs));
    let mut jset = JoinSet::new();
    let mut results = input.iter().map(|_| None).collect::<Vec<_>>();
    for item in input.into_iter().enumerate() {
        let semaphore = semaphore.clone();
        let permit = semaphore.acquire_owned().await?;
        jset.spawn(async move {
            let _permit = permit;
            let res = f(item.1).await?;
            Ok((item.0, res))
        });
    }
    while let Some(result) = jset.join_next().await {
        let err: eyre::Report = match result {
            Ok(Ok((i, result))) => {
                results[i] = Some(result);
                continue;
            }
            Ok(Err(e)) => e,
            Err(e) => e.into(),
        };
        jset.abort_all();
        jset.join_all().await;
        return Err(err);
    }
    Ok(results.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::test;

    #[test]
    async fn test_parallel() {
        let input = vec![1, 2, 3, 4, 5];
        let results = parallel(input, |x| async move { Ok(x * 2) }).await.unwrap();
        assert_eq!(results, vec![2, 4, 6, 8, 10]);
    }
}
