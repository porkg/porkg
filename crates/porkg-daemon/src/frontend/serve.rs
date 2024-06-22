use std::{net::ToSocketAddrs, path::PathBuf};

use anyhow::Context;
use axum::extract::connect_info::Connected;
use hyper::{
    body::Incoming,
    rt::{Read, Write},
    Request,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use porkg_private::future::OptionalFutureExt as _;
use tokio::net::{TcpListener, TcpStream, UnixListener, UnixStream};
use tokio_util::sync::CancellationToken;
use tower_service::Service;

pub struct UnixHostOptions {
    pub path: PathBuf,
}

pub struct TcpHostOptions {
    pub listen: Vec<std::net::SocketAddr>,
}

pub struct HostOptions {
    pub unix: UnixHostOptions,
    pub tcp: Option<TcpHostOptions>,
}

enum Client {
    Tcp { stream: TokioIo<TcpStream> },
    Unix { stream: TokioIo<UnixStream> },
}

impl From<(UnixStream, tokio::net::unix::SocketAddr)> for Client {
    fn from(value: (UnixStream, tokio::net::unix::SocketAddr)) -> Self {
        Self::Unix {
            stream: TokioIo::new(value.0),
        }
    }
}

impl From<(TcpStream, std::net::SocketAddr)> for Client {
    fn from(value: (TcpStream, std::net::SocketAddr)) -> Self {
        Self::Tcp {
            stream: TokioIo::new(value.0),
        }
    }
}

impl hyper::rt::Read for Client {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: hyper::rt::ReadBufCursor<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match self.get_mut() {
            Client::Tcp { stream, .. } => Read::poll_read(std::pin::pin!(stream), cx, buf),
            Client::Unix { stream, .. } => Read::poll_read(std::pin::pin!(stream), cx, buf),
        }
    }
}

impl Write for Client {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        match self.get_mut() {
            Client::Tcp { stream, .. } => Write::poll_write(std::pin::pin!(stream), cx, buf),
            Client::Unix { stream, .. } => Write::poll_write(std::pin::pin!(stream), cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match self.get_mut() {
            Client::Tcp { stream, .. } => Write::poll_flush(std::pin::pin!(stream), cx),
            Client::Unix { stream, .. } => Write::poll_flush(std::pin::pin!(stream), cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match self.get_mut() {
            Client::Tcp { stream, .. } => Write::poll_shutdown(std::pin::pin!(stream), cx),
            Client::Unix { stream, .. } => Write::poll_shutdown(std::pin::pin!(stream), cx),
        }
    }
}

#[derive(Debug, Clone)]
enum ClientInfo {
    Tcp,
    Unix,
}

impl Connected<&Client> for ClientInfo {
    fn connect_info(target: &Client) -> Self {
        match target {
            Client::Tcp { .. } => ClientInfo::Tcp,
            Client::Unix { .. } => ClientInfo::Unix,
        }
    }
}

pub async fn serve(
    settings: HostOptions,
    router: axum::Router,
    cancellation_token: CancellationToken,
) -> anyhow::Result<()>
where
{
    let socket = settings.unix;
    if tokio::fs::try_exists(&socket.path).await? {
        tracing::trace!(socket_path = ?&socket.path, "cleaning up previous socket");
        tokio::fs::remove_file(&socket.path)
            .await
            .with_context(|| format!("failed to bind to {:?}", &socket.path))?;
    }

    tracing::trace!(socket_path = ?&socket.path, "binding");
    let unix = UnixListener::bind(&socket.path)?;

    let tcp = if let Some(tcp) = settings.tcp.as_ref() {
        let mut socket_addrs = Vec::with_capacity(tcp.listen.len());
        for bind in tcp.listen.iter() {
            for addr in bind.to_socket_addrs()? {
                socket_addrs.push(addr);
            }
        }

        if socket_addrs.is_empty() {
            None
        } else {
            tracing::trace!("binding tcp to {:?}", &socket_addrs);
            Some(TcpListener::bind(&socket_addrs[..]).await?)
        }
    } else {
        None
    };

    let mut make = router.into_make_service_with_connect_info::<ClientInfo>();

    loop {
        let tcp = tcp.as_ref().map(|v| v.accept()).unwrap_future();
        let socket = tokio::select! {
            result = unix.accept() =>  result.map(Into::into),
            result = tcp => result.map(Into::into),
            _ = cancellation_token.cancelled() => {
                println!("f");
                break Ok(())
                }
        };

        let socket = match socket {
            Err(e) if is_connection_error(&e) => continue,
            other => other,
        }?;

        let tower_service = make.call(&socket).await.unwrap_or_else(|err| match err {});

        tokio::spawn(async move {
            let hyper_service = hyper::service::service_fn(move |request: Request<Incoming>| {
                tower_service.clone().call(request)
            });

            if let Err(err) = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(socket, hyper_service)
                .await
            {
                tracing::info!(?err, "error responding to request")
            }
        });
    }
}

fn is_connection_error(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
    )
}
