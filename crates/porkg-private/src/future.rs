use std::{future::Future, pin::Pin};

pin_project_lite::pin_project! {
    struct OptionalFuture<T, F: Future<Output = T>> {
        #[pin]
        o: Option<F>
    }
}

mod sealed {
    pub trait Sealed {}
}

impl<T, F: Future<Output = T>> Future for OptionalFuture<T, F> {
    type Output = T;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match self.project().o.as_pin_mut() {
            Some(v) => Future::poll(v, cx),
            None => std::task::Poll::Pending,
        }
    }
}

/// Extension methods for `Option<Future>`.
pub trait OptionalFutureExt: sealed::Sealed {
    type Output;

    /// Creates a future that unwraps and awaits the future in the option, while never resolving if the option is None.
    fn unwrap_future(self) -> (impl Send + Future<Output = Self::Output>);
}

impl<T, F: Send + Future<Output = T>> sealed::Sealed for Option<F> {}
impl<T, F: Send + Future<Output = T>> OptionalFutureExt for Option<F> {
    type Output = T;

    fn unwrap_future(self) -> (impl Send + Future<Output = Self::Output>) {
        OptionalFuture { o: self }
    }
}

#[cfg(test)]
mod test {
    use std::{future::Future, time::Duration};

    use super::OptionalFutureExt as _;

    struct Completed;

    impl Future for Completed {
        type Output = ();

        fn poll(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            std::task::Poll::Ready(())
        }
    }

    #[tokio::test]
    async fn unwrap_future_some() {
        let result =
            tokio::time::timeout(Duration::from_millis(100), Some(Completed).unwrap_future()).await;
        assert_eq!(Ok(()), result);
    }

    #[tokio::test]
    async fn unwrap_future_none() {
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            None::<Completed>.unwrap_future(),
        )
        .await;
        assert!(result.is_err())
    }
}
